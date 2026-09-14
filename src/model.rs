mod units {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Score(u8);
    impl TryFrom<i64> for Score {
        type Error = &'static str;
        fn try_from(value: i64) -> Result<Self, Self::Error> {
            if !(1..=10).contains(&value) {
                return Err("value must be 1..=10");
            }

            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "value is already validated to be 1..=10"
            )]
            let value = u8::try_from(value)
                .map_err(|_e| "should never happen: value validated to be 1..=10")?;
            Ok(Self(value))
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Weight(u8);
    impl TryFrom<i64> for Weight {
        type Error = &'static str;
        fn try_from(value: i64) -> Result<Self, Self::Error> {
            if !(0..=100).contains(&value) {
                return Err("value must be 0..=100");
            }

            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "value is already validated to be 0..=100"
            )]
            let value = u8::try_from(value)
                .map_err(|_e| "should never happen: value validated to be 0..=100")?;
            Ok(Self(value))
        }
    }

    pub fn mult_score_weight(score: Score, weight: Weight) -> u32 {
        (u32::from(score.0)).saturating_mul(u32::from(weight.0))
    }
}

pub mod parser {
    use serde::Deserialize;
    use std::collections::{BTreeMap, HashSet};

    use super::units::{Score, Weight, mult_score_weight};

    #[derive(Debug)]
    pub struct Config {
        weights: BTreeMap<Attribute, Weight>,
    }
    impl Config {
        pub fn parse(text: &str) -> Result<Self, Box<dyn std::error::Error>> {
            let toml: ConfigToml = toml::from_str(text)?;
            let weights: Result<BTreeMap<Attribute, Weight>, &str> = toml
                .weights
                .into_iter()
                .map(|(k, v)| Weight::try_from(v).map(|w| (Attribute(k), w)))
                .collect();

            let weights: BTreeMap<Attribute, Weight> = weights?;
            Ok(Self { weights })
        }

        pub fn validate(&self, attributes: &BTreeMap<String, i64>) -> Result<(), &str> {
            if self.weights.len() != attributes.len() {
                return Err("Attributes do not match");
            }

            let attrs: Vec<&String> = attributes.keys().collect();
            for (i, w) in self.weights.keys().enumerate() {
                // if w.0 != *attrs[i] {
                let a = attrs.get(i).ok_or("Failed to index attributes")?;
                if w.0 != **a {
                    return Err("Attributes do not match");
                }
            }

            Ok(())
        }
    }

    #[derive(Debug)]
    pub struct Project {
        name: ProjectName,
        info: Info,
        attributes: BTreeMap<Attribute, Score>,
        score: u32,
    }
    impl Project {
        pub fn parse(
            text: &str,
            name: &str,
            config: &Config,
            project_names: &ProjectNames,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let name = project_names.mint(name)?;

            let toml: ProjectToml = toml::from_str(text)?;
            let repo_link = if toml.info.repo_link.is_empty() {
                None
            } else {
                Some(toml.info.repo_link)
            };
            let builds_on: Result<Vec<ProjectName>, String> = toml
                .info
                .builds_on
                .into_iter()
                .map(|p| project_names.mint(&p))
                .collect();
            let builds_on: Vec<ProjectName> = builds_on?;
            let info = Info {
                description: toml.info.description,
                repo_link,
                tags: toml.info.tags,
                builds_on,
            };

            config.validate(&toml.attributes)?;
            let attributes: Result<BTreeMap<Attribute, Score>, &str> = toml
                .attributes
                .into_iter()
                .map(|(k, v)| Score::try_from(v).map(|s| (Attribute(k), s)))
                .collect();
            let attributes: BTreeMap<Attribute, Score> = attributes?;

            let score = Self::score(&attributes, config)?;

            Ok(Self {
                name,
                info,
                attributes,
                score,
            })
        }

        pub fn project_name(&self) -> &str {
            &self.name.0
        }

        pub fn repo_link(&self) -> Option<&str> {
            self.info.repo_link.as_deref()
        }

        fn score(attributes: &BTreeMap<Attribute, Score>, config: &Config) -> Result<u32, String> {
            let mut total = 0u32;
            for (attribute, score) in attributes {
                let weight = config
                    .weights
                    .get(attribute)
                    .ok_or("Should never happen: failed to get key")?;
                total = total.saturating_add(mult_score_weight(*score, *weight));
            }
            Ok(total)
        }
    }

    #[derive(Debug)]
    pub struct Info {
        description: String,
        repo_link: Option<String>,
        tags: Vec<String>,
        builds_on: Vec<ProjectName>,
    }

    #[derive(Deserialize)]
    struct ConfigToml {
        weights: BTreeMap<String, i64>,
    }

    #[derive(Deserialize)]
    struct ProjectToml {
        info: InfoToml,
        attributes: BTreeMap<String, i64>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct InfoToml {
        description: String,
        repo_link: String,
        tags: Vec<String>,
        builds_on: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Attribute(String);
    impl std::borrow::Borrow<str> for Attribute {
        fn borrow(&self) -> &str {
            &self.0
        }
    }

    #[derive(Debug)]
    struct ProjectName(String);

    pub struct ProjectNames(HashSet<String>);
    impl ProjectNames {
        pub const fn new(names: HashSet<String>) -> Self {
            Self(names)
        }

        fn mint(&self, name: &str) -> Result<ProjectName, String> {
            if self.0.contains(name) {
                return Ok(ProjectName(name.to_string()));
            }

            Err(format!("ProjectName {name} not found"))
        }
    }
}

pub mod context {
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

        let our_response = serde_json::from_str::<T>(&content.text)?;
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
}
