use super::parser::{Config, Project};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::from_str;
use std::fmt::Write as _;

const HISTORY_SCRIPT: &str = "src/history.sh";
const LOOKBACK_DAYS: &str = "7";

#[derive(Debug, Deserialize)]
pub struct PrioModelOutput {
    order: Vec<PrioItem>,
    notes: String,
}

#[derive(Debug, Deserialize)]
struct PrioItem {
    project_name: String, // todo: should validate attribute on parse back from model?
    priority: u8,
    justification: String,
}

#[derive(Deserialize, Debug)]
pub struct ProjectModelOutput {
    opinions: std::collections::BTreeMap<String, Opinion>, // todo: should validate attribute on parse back from model?
    notes: String,
}

#[derive(Deserialize, Debug)]
struct Opinion {
    reason: String,
    score: u8,
}

// in case the model puts the json in markdown fences, would break parsing
fn strip_markdown_fence(text: &str) -> &str {
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .unwrap_or(text)
        .trim_start();
    text.strip_suffix("```").unwrap_or(text).trim_end()
}

pub fn parse_model_output<T: DeserializeOwned>(
    raw_output: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let model_response = serde_json::from_str::<ModelResponse>(raw_output)?;
    let contents: Vec<Content> = model_response
        .steps
        .into_iter()
        .filter_map(|s| s.content)
        .flatten()
        .collect();

    if contents.len() != 1 {
        return Err("Expected only 1 content block in the response".into());
    }
    let content = contents
        .first()
        .ok_or("Should never happen, validated contents len")?;

    let our_response = serde_json::from_str::<T>(strip_markdown_fence(&content.text))?;
    Ok(our_response)
}

pub struct ProjectBundle {
    context: ProjectContext,
    output: ProjectModelOutput,
}
impl ProjectBundle {
    pub const fn new(context: ProjectContext, output: ProjectModelOutput) -> Self {
        Self { context, output }
    }

    pub fn create_prompt(bundles: &[Self], config: &Config) -> String {
        let mut prompt = String::from(
            "You are an agent working on an assignment that synthesizes user scored attributes, commit history, and information aggregated by per-project agents one level below you. Your job is to look into and across all bundled project information to recommend a priority list for what the user should work on next. If there is no history, then the project has not been started yet.\n\n",
        );

        let _ = write!(
            prompt,
            "Attribute weights (higher = more important to the user):
            {config:#?}\n\n"
        );

        for project in bundles {
            let _ = write!(
                prompt,
                "
                Project: {:#?}\n\
                History: {:#?}\n\
                SubAgent Opinions: {:#?}\n\
                SubAgent Notes: {:#?}\n\n\
                ",
                project.context.project,
                project.context.history,
                project.output.opinions,
                project.output.notes
            );
        }

        let _ = write!(
            prompt,
            "
            \nUsing all project information, independently derive a priority score for each project (bounded [0, 100]), where a higher score means the project should be worked on sooner. The gap between two projects' scores should reflect how close a call it is — close scores mean it's a toss-up, a large gap means one is clearly more urgent. List `order` sorted from highest score to lowest.

            Each project's `score` field reflects the user's original attribute scores combined with the attribute weights above; it is not on the same [0, 100] scale as the priority you derive here, so don't try to reconcile the two numerically. Instead, use the attribute weights above together with each project's SubAgent Opinions to judge whether the subagent's rederived attribute profile still supports the user's original ranking, and explain your priority in those terms.

            Structure your output as json:
            {{
                \"order\": [
                    {{
                        \"project_name\": \"project_a\",
                        \"priority\": 60,
                        \"justification\": \"justification_a\"
                    }},
                    {{
                        \"project_name\": \"project_b\",
                        \"priority\": 30,
                        \"justification\": \"justification_b\"
                    }}
                ],
                \"notes\": \"important notes to pass up to the user, reflections on the process and prompts\"
            }}
            "
        );

        prompt
    }
}

#[derive(Debug)]
pub struct ProjectContext {
    project: Project,
    history: Option<Vec<Commit>>,
}
impl ProjectContext {
    pub fn enrich(project: Project) -> Self {
        let history = Self::get_history(&project).ok();
        Self { project, history }
    }

    pub fn create_prompt(&self) -> String {
        format!(
            "You are an agent working on an assignment that synthesizes user scored attributes and commit history to recommend what the user should work on next. Your job is to look at this one project and synthesize information for an aggregation agent to use in order to recommend a priority list for what the user should work on next.
            If there is no history, then the project has not been started yet.

            Project: {:#?}
            History: {:#?}

            Using the commit information, independently derive attribute scores (bounded [1, 10]). If it disagrees with the user score, justify your score using the semantic commit info.

            Structure your output as json:
            {{
                \"opinions\": {{
                    \"attribute_a\": {{
                        \"reason\": \"opinion_a\",
                        \"score\": 4
                    }},
                    \"attribute_c\": {{
                        \"reason\": \"opinion_c\",
                        \"score\": 8
                    }}
                }},
                \"notes\": \"important notes to pass up to aggregator agent\"
            }}
            ",
            self.project,
            self.history,
        )
    }

    fn get_history(project: &Project) -> Result<Vec<Commit>, Box<dyn std::error::Error>> {
        let repo_link = project.repo_link();
        let Some(repo_link) = repo_link else {
            return Err("No repo link".into());
        };
        let Some(project_name) = repo_link.split('/').next_back() else {
            return Err("No project name in repo link".into());
        };

        let output = std::process::Command::new(HISTORY_SCRIPT)
            .args([project_name, LOOKBACK_DAYS])
            .output()?;
        let output = String::from_utf8(output.stdout)?;
        let mut commits = Vec::<Commit>::new();
        for line in output.lines() {
            let commit: Commit = from_str(line)?;
            commits.push(commit);
        }
        Ok(commits)
    }
}

#[derive(Deserialize, Debug)]
struct Commit {
    hash: String,
    branch: String,
    subject: String,
    date: DateTime<Utc>,
    link: String,
    #[serde(flatten)]
    delta: Delta,
}

#[derive(Deserialize, Debug)]
struct Delta {
    additions: u32,
    deletions: u32,
}

#[derive(Deserialize, Debug)]
struct ModelResponse {
    steps: Vec<Step>,
}

#[derive(Deserialize, Debug)]
struct Step {
    content: Option<Vec<Content>>,
}

#[derive(Deserialize, Debug)]
struct Content {
    text: String,
}
