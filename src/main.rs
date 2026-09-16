mod model;

use std::fs;
use std::ops::Add;

use crate::model::context::{
    PrioModelOutput, ProjectBundle, ProjectContext, ProjectModelOutput, Usage, parse_model_output,
};
use crate::model::parser::{Config, Project, ProjectNames};

const API_KEY_ENV_VAR: &str = "OPENROUTER_API_KEY";

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
    let Ok(api_key) = std::env::var(API_KEY_ENV_VAR) else {
        eprintln!("{API_KEY_ENV_VAR} not found");
        return;
    };
    let mut projects = Vec::<ProjectBundle>::new();
    let mut project_usage = std::collections::BTreeMap::<String, Usage>::new(); // todo: validate project_name keys
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
        let (project_output, usage) = match parse_model_output::<ProjectModelOutput>(
            &project_response,
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to parse project: {e}");
                eprintln!("{project_response}");
                continue;
            }
        };
        project_usage.insert(project.project_name().to_string(), usage);
        projects.push(ProjectBundle::new(project, project_output));
    }

    let prompt = ProjectBundle::create_prompt(&projects, &config);
    println!(" (╭ರ_•́) prio time");
    let prio_response = match model::request::handle_prompt(&api_key, &prompt) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Invalid response for prio: {e}");
            return;
        }
    };
    let (prio_output, prio_usage) = match parse_model_output::<PrioModelOutput>(&prio_response) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to parse prio: {e}");
            eprintln!("{prio_response}");
            return;
        }
    };

    dbg!(prio_output);
    print_usage_summary(&project_usage, &prio_usage);
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

fn print_usage_summary(project_usage: &std::collections::BTreeMap<String, Usage>, prio_usage: &Usage) {
    let (p_prompt, p_reasoning, p_total, p_cost) = project_usage.values().fold(
        (0u32, 0u32, 0u32, 0f64),
        |(prompt, reasoning, total, cost), u| {
            (
                prompt.saturating_add(u.prompt_tokens),
                reasoning.saturating_add(u.completion_tokens_details.reasoning_tokens),
                total.saturating_add(u.total_tokens),
                cost.add(u.cost),
            )
        },
    );
    let prio_reasoning = prio_usage.completion_tokens_details.reasoning_tokens;

    println!("\n(っ$_$)╮=͟͟͞͞💸 usage summary");
    println!(
        "\tprojects  prompt={p_prompt} reasoning={p_reasoning} total={p_total} cost=${p_cost:.4}"
    );
    println!(
        "\tprio      prompt={} reasoning={prio_reasoning} total={} cost=${:.4}",
        prio_usage.prompt_tokens, prio_usage.total_tokens, prio_usage.cost
    );
    println!(
        "\ttotal     prompt={} reasoning={} total={} cost=${:.4}",
        p_prompt.saturating_add(prio_usage.prompt_tokens),
        p_reasoning.saturating_add(prio_reasoning),
        p_total.saturating_add(prio_usage.total_tokens),
        p_cost.add(prio_usage.cost)
    );
}
