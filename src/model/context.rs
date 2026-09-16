use super::parser::{Config, Project};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::fmt::Write as _;

const OWNER: &str = "moleraat";
const LOOKBACK_DAYS: i64 = 7;

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

pub fn parse_model_output<T: DeserializeOwned>(
    raw_output: &str,
) -> Result<(T, Usage), Box<dyn std::error::Error>> {
    let model_response = serde_json::from_str::<ModelResponse>(raw_output)?;
    if model_response.choices.len() != 1 {
        return Err("Expected only 1 choice in the response".into());
    }
    let choice = model_response
        .choices
        .into_iter()
        .next()
        .ok_or("Should never happen, validated choices len")?;

    let our_response = serde_json::from_str::<T>(&choice.message.content)?;
    Ok((our_response, model_response.usage))
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

    pub fn project_name(&self) -> &str {
        self.project.project_name()
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
        let Some(repo) = repo_link.split('/').next_back() else {
            return Err("No project name in repo link".into());
        };

        let since = Utc::now()
            .checked_sub_signed(Duration::days(LOOKBACK_DAYS))
            .ok_or("Failed to compute lookback date")?
            .format("%Y-%m-%d");

        let branches: Vec<GhBranch> = gh_api(&format!("repos/{OWNER}/{repo}/branches"))?;
        let mut commits = Vec::<Commit>::new();
        for branch in branches {
            let summaries: Vec<GhCommitSummary> = gh_api(&format!(
                "repos/{OWNER}/{repo}/commits?sha={}&since={since}",
                branch.name
            ))?;
            for summary in summaries {
                let detail: GhCommitDetail =
                    gh_api(&format!("repos/{OWNER}/{repo}/commits/{}", summary.sha))?;
                commits.push(Commit {
                    hash: detail.sha,
                    branch: branch.name.clone(),
                    subject: detail.commit.message,
                    date: detail.commit.author.date,
                    link: detail.html_url,
                    delta: Delta {
                        additions: detail.stats.additions,
                        deletions: detail.stats.deletions,
                    },
                });
            }
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

fn gh_api<T: DeserializeOwned>(path: &str) -> Result<T, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("gh")
        .args(["api", path])
        .output()?;
    let body = String::from_utf8(output.stdout)?;
    Ok(serde_json::from_str(&body)?)
}

#[derive(Deserialize)]
struct GhBranch {
    name: String,
}

#[derive(Deserialize)]
struct GhCommitSummary {
    sha: String,
}

#[derive(Deserialize)]
struct GhCommitDetail {
    sha: String,
    html_url: String,
    commit: GhCommitDetailInner,
    stats: Delta,
}

#[derive(Deserialize)]
struct GhCommitDetailInner {
    message: String,
    author: GhAuthor,
}

#[derive(Deserialize)]
struct GhAuthor {
    date: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
struct ModelResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize, Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
    pub cost: f64,
    pub completion_tokens_details: CompletionTokensDetails,
}

#[derive(Deserialize, Debug)]
pub struct CompletionTokensDetails {
    pub reasoning_tokens: u32,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
}

#[derive(Deserialize, Debug)]
struct Message {
    content: String,
}
