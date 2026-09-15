mod model;

use std::fs;

use crate::model::context::{
    PrioModelOutput, ProjectBundle, ProjectContext, ProjectModelOutput, parse_model_output,
};
use crate::model::parser::{Config, Project, ProjectNames};

fn main() {
    // Parse config file
    let config_path = std::path::Path::new("config.toml");
    let Ok(raw_config) = fs::read_to_string(config_path) else {
        eprintln!("Invalid config path");
        return;
    };
    let Ok(config) = Config::parse(&raw_config) else {
        eprintln!("Invalid config file");
        return;
    };

    // Get ProjectNames
    let project_path = std::path::Path::new("projects/");
    let Ok(read_dirs_iter) = fs::read_dir(project_path) else {
        eprintln!("Invalid project path");
        return;
    };
    let read_dirs = read_dirs_iter.collect::<Vec<_>>();
    let mut project_names = std::collections::HashSet::<String>::new();
    for dir_entry in &read_dirs {
        let project_entry = match dir_entry {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Invalid directory entry: {e}");
                continue;
            }
        };
        let project_name = project_entry.file_name();
        project_names.insert(project_name.to_string_lossy().into_owned());
    }
    let project_names = ProjectNames::new(project_names);

    // Parse project files, call llm
    let Ok(api_key) = std::env::var("MODEL_API_KEY") else {
        eprintln!("MODEL_API_KEY not found");
        return;
    };
    let mut projects = Vec::<ProjectBundle>::new();
    for dir_entry in read_dirs {
        let project = match handle_file(dir_entry, &config, &project_names) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Invalid project: {e}");
                continue;
            }
        };
        let prompt = project.create_prompt();
        let project_response = match model::request::handle_prompt(&api_key, &prompt) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Invalid response for project: {e}");
                continue;
            }
        };
        let project_output = match parse_model_output::<ProjectModelOutput>(&project_response) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to parse project: {e}");
                continue;
            }
        };
        projects.push(ProjectBundle::new(project, project_output));
    }

    let prompt = ProjectBundle::create_prompt(&projects, &config);
    let prio_response = match model::request::handle_prompt(&api_key, &prompt) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Invalid response for prio: {e}");
            return;
        }
    };
    let prio_output = match parse_model_output::<PrioModelOutput>(&prio_response) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to parse prio: {e}");
            return;
        }
    };

    dbg!(prio_output);

    // ping
}

fn handle_file(
    dir_entry: Result<std::fs::DirEntry, std::io::Error>,
    config: &Config,
    project_names: &ProjectNames,
) -> Result<ProjectContext, Box<dyn std::error::Error>> {
    let project_entry = dir_entry?;
    let project_name = project_entry.file_name().to_string_lossy().into_owned();
    println!("(⌐■_■) {project_name}");

    let raw_project = fs::read_to_string(project_entry.path())?;
    let project = Project::parse(&raw_project, &project_name, config, project_names)?;

    let project = ProjectContext::enrich(project);
    Ok(project)
}

