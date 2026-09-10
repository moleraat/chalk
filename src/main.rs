mod model;

use std::error::Error;
use std::fs::DirEntry;
use std::io::Error as IoError;
use std::path::Path;
use std::{collections::HashSet, fs};

use crate::model::context::ProjectContext;
use crate::model::parser::{Config, Project, ProjectNames};

fn main() {
    // Parse config file
    let config_path = Path::new("config.toml");
    let Ok(raw_config) = fs::read_to_string(config_path) else {
        eprintln!("Invalid config path");
        return
    };
    let Ok(config) = Config::parse(&raw_config) else {
        eprintln!("Invalid config file");
        return
    };

    // Get ProjectNames
    let project_path = Path::new("projects/");
    let Ok(read_dirs_iter) = fs::read_dir(project_path) else {
        eprintln!("Invalid project path");
        return
    };
    let read_dirs = read_dirs_iter.collect::<Vec<_>>();
    let mut project_names = HashSet::<String>::new();
    for dir_entry in &read_dirs {
        let project_entry = match dir_entry {
            Ok(p) => p,
            Err(e) => {
                println!("Invalid directory entry: {e}");
                continue;
            }
        };
        let project_name = project_entry.file_name();
        project_names.insert(project_name.to_string_lossy().into_owned());
    }
    let project_names = ProjectNames::new(project_names);

    // Parse project files, call llm
    for dir_entry in read_dirs {
        let prompt = match handle_file(dir_entry, &config, &project_names) {
            Ok(p) => p,
            Err(e) => {
                println!("Invalid project: {e}");
                continue;
            }
        };
    }

    // fetch recent repo activity
    // feed bundles to llm
    // get structured recommendations & observations
    // write llm output
    // ping
}

fn handle_file(
    dir_entry: Result<DirEntry, IoError>,
    config: &Config,
    project_names: &ProjectNames,
) -> Result<String, Box<dyn Error>> {
    let project_entry = dir_entry?;
    let project_name = project_entry.file_name().to_string_lossy().into_owned();

    let raw_project = fs::read_to_string(project_entry.path())?;
    let project = Project::parse(&raw_project, &project_name, config, project_names)?;

    let project = ProjectContext::enrich(project);
    let prompt = project.create_prompt();
    Ok(prompt)
}
