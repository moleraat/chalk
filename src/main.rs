mod model;

use std::fs;
use std::ops::Add;

use crate::model::context::{ProjectBundle, ProjectContext};
use crate::model::parser::{Config, Project, ProjectNames};
use crate::model::request::Usage;

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
        let (project_output, usage) = match project.request_opinion(&api_key) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to get opinion for project: {e}");
                continue;
            }
        };

        project_usage.insert(project.project_name().to_string(), usage);
        projects.push(ProjectBundle::new(project, project_output));
    }

    println!(" (╭ರ_•́) prio time");
    let (prio_output, prio_usage) =
        match ProjectBundle::request_priority(&projects, &config, &api_key) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Failed to get priority: {e}");
                return;
            }
        };

    println!("{prio_output}");
    if let Err(e) = prio_output.post_as_issue() {
        eprintln!("Failed to post report issue: {e}");
    }
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

fn print_usage_summary(
    project_usage: &std::collections::BTreeMap<String, Usage>,
    prio_usage: &Usage,
) {
    println!("\n(っ$_$)╮=͟͟͞͞💸 usage summary");

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
    let p_output = p_total.saturating_sub(p_prompt).saturating_sub(p_reasoning);
    let (p_prompt_percent, p_reasoning_percent, p_output_percent) = (
        (f64::from(p_prompt) / f64::from(p_total)) * 100.0,
        (f64::from(p_reasoning) / f64::from(p_total)) * 100.0,
        (f64::from(p_output) / f64::from(p_total)) * 100.0,
    );
    println!(
        "\tprojects: {p_total} tokens | {p_prompt_percent:.1}% prompt, {p_reasoning_percent:.1}% reasoning, {p_output_percent:.1}% output | ${p_cost:.4}"
    );

    let (prio_prompt, prio_reasoning, prio_total, prio_cost) = (
        prio_usage.prompt_tokens,
        prio_usage.completion_tokens_details.reasoning_tokens,
        prio_usage.total_tokens,
        prio_usage.cost,
    );
    let prio_output = prio_total
        .saturating_sub(prio_prompt)
        .saturating_sub(prio_reasoning);
    let (prio_prompt_percent, prio_reasoning_percent, prio_output_percent) = (
        (f64::from(prio_prompt) / f64::from(prio_total)) * 100.0,
        (f64::from(prio_reasoning) / f64::from(prio_total)) * 100.0,
        (f64::from(prio_output) / f64::from(prio_total)) * 100.0,
    );
    println!(
        "\tprio: {prio_total} tokens | {prio_prompt_percent:.1}% prompt, {prio_reasoning_percent:.1}% reasoning, {prio_output_percent:.1}% output | ${prio_cost:.4}"
    );

    let (project_percent, prio_percent) = (
        (f64::from(p_total) / f64::from(p_total.saturating_add(prio_total))) * 100.0,
        (f64::from(prio_total) / f64::from(p_total.saturating_add(prio_total))) * 100.0,
    );
    println!(
        "\ttotal: {project_percent:.1}% projects, {prio_percent:.1}% prio | ${:.4}",
        p_cost.add(prio_cost)
    );
}
