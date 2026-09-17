use super::github::{self, Commit, FALLBACK_COMMIT_COUNT, LOOKBACK_DAYS};
use super::parser::{Config, Project};
use super::request::{self, Usage};
use chrono::Utc;
use serde::Deserialize;
use std::fmt::Write as _;

// prio prompt and response ----------------------------------------------------
pub struct ProjectBundle {
    context: ProjectContext,
    output: ProjectModelOutput,
}
impl ProjectBundle {
    pub const fn new(context: ProjectContext, output: ProjectModelOutput) -> Self {
        Self { context, output }
    }

    pub fn request_priority(
        bundles: &[Self],
        config: &Config,
        api_key: &str,
    ) -> Result<(PrioModelOutput, Usage), Box<dyn std::error::Error>> {
        let prompt = Self::create_prompt(bundles, config);
        request::request_and_parse(api_key, &prompt)
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
                Days since last commit: {:?}\n\
                SubAgent Opinions: {:#?}\n\
                SubAgent Notes: {:#?}\n\n\
                ",
                project.context.project,
                project.context.days_since_last_commit(),
                project.output.opinions,
                project.output.notes
            );
        }

        let bundle_count = bundles.len();
        let _ = write!(
            prompt,
            "
            \nUsing all project information, independently derive a priority score for each project (bounded [0, 100]), where a higher score means the project should be worked on sooner. The gap between two projects' scores should reflect how close a call it is — close scores mean it's a toss-up, a large gap means one is clearly more urgent. List `order` sorted from highest score to lowest.

            There are exactly {bundle_count} projects listed above. Your `order` array MUST include all {bundle_count} of them, each exactly once — do not omit, merge, or duplicate any project.

            Each project's `score` field reflects the user's original attribute scores combined with the attribute weights above; it is not on the same [0, 100] scale as the priority you derive here, so don't try to reconcile the two numerically. Instead, use the attribute weights above together with each project's SubAgent Opinions to judge whether the subagent's rederived attribute profile still supports the user's original ranking, and explain your priority in those terms.

            Carefully structure your output as valid json:
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

// projct prompt and response --------------------------------------------------
#[derive(Debug)]
pub struct ProjectContext {
    project: Project,
    history: Option<Vec<Commit>>,
}
impl ProjectContext {
    pub fn enrich(project: Project) -> Self {
        let history = github::fetch_history(&project).ok();
        Self { project, history }
    }

    pub fn project_name(&self) -> &str {
        self.project.project_name()
    }

    pub const fn score(&self) -> u32 {
        self.project.score()
    }

    pub fn days_since_last_commit(&self) -> Option<i64> {
        self.history.as_deref().and_then(github::days_since_last_commit)
    }

    pub fn request_opinion(
        &self,
        api_key: &str,
    ) -> Result<(ProjectModelOutput, Usage), Box<dyn std::error::Error>> {
        let prompt = self.create_prompt();
        request::request_and_parse(api_key, &prompt)
    }

    pub fn create_prompt(&self) -> String {
        format!(
            "You are an agent working on an assignment that synthesizes user scored attributes and commit history to recommend what the user should work on next. Your job is to look at this one project and synthesize information for an aggregation agent to use in order to recommend a priority list for what the user should work on next.
            History is either activity from past {LOOKBACK_DAYS} days, or the last {FALLBACK_COMMIT_COUNT} commits before that window. If there is no history, then the project has not been started yet.

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
}

// parsed prio model response
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

impl std::fmt::Display for PrioModelOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "## Priority order\n")?;
        for (i, item) in self.order.iter().enumerate() {
            writeln!(
                f,
                "{}. **{}** ({}) : {}",
                i.saturating_add(1),
                item.project_name,
                item.priority,
                item.justification
            )?;
        }
        write!(f, "\n**Notes:** {}", self.notes)
    }
}

impl PrioModelOutput {
    pub fn post_as_issue(&self) -> Result<(), Box<dyn std::error::Error>> {
        let title = format!("☕︎ Chalk Priority Report — {}", Utc::now().format("%Y-%m-%d"));
        github::create_issue(&title, &self.to_string())
    }
}

// parsed project model response
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


