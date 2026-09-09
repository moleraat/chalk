mod units {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Score(u8);
    impl TryFrom<i64> for Score {
        type Error = &'static str;
        fn try_from(value: i64) -> Result<Self, Self::Error> {
            if !(1..=10).contains(&value) {
                return Err("value must be 1..=10");
            }

            Ok(Score(value as u8))
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

            Ok(Weight(value as u8))
        }
    }

    pub fn mult_score_weight(score: &Score, weight: &Weight) -> u32 {
        (score.0 as u32).saturating_mul(weight.0 as u32)
    }
}

pub mod parser {
    use serde::Deserialize;
    use std::borrow::Borrow;
    use std::collections::BTreeMap;
    use std::collections::HashSet;
    use std::error::Error;

    use super::units::{Score, Weight, mult_score_weight};

    pub struct ProjectBundle {
        project: Project,
        score: u32,
        repo_context: String,
    }

    pub struct Config {
        weights: BTreeMap<Attribute, Weight>,
    }
    impl Config {
        pub fn parse(text: &str) -> Result<Self, Box<dyn Error>> {
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
                if w.0 != *attrs[i] {
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
        ) -> Result<Self, Box<dyn Error>> {
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

            let score = Self::score(&attributes, config);

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

        fn score(attributes: &BTreeMap<Attribute, Score>, config: &Config) -> u32 {
            let mut total = 0u32;
            for (attribute, score) in attributes {
                let weight = config.weights.get(attribute).unwrap();
                total += mult_score_weight(score, weight);
            }
            total
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
    impl Borrow<str> for Attribute {
        fn borrow(&self) -> &str {
            &self.0
        }
    }

    #[derive(Debug)]
    struct ProjectName(String);

    pub struct ProjectNames(HashSet<String>);
    impl ProjectNames {
        pub fn new(names: HashSet<String>) -> Self {
            Self(names)
        }

        fn mint(&self, name: &str) -> Result<ProjectName, String> {
            if self.0.contains(name) {
                return Ok(ProjectName(name.to_string()));
            };

            Err(format!("ProjectName {name} not found"))
        }
    }
}

pub mod context {
    use super::parser::Project;
    use chrono::{DateTime, Utc};
    use serde::Deserialize;
    use serde_json::from_str;
    use std::error::Error;
    use std::process::Command;

    const HISTORY_SCRIPT: &str = "src/history.sh";
    const LOOKBACK_DAYS: &str = "7";

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
                "You are an agent working on a project that synthesizes user scored attributes and commit history to recommend what the user should work on next. Your job is to look at this one project and synthesize information for an aggregation agent to use in order to recommend a priority list for what the user should work on next.
                If there is no history, then the project has not been started yet.

                Project: {:#?}
                History: {:#?}

                Using the commit information, derive the current activity level of the project.
                Using the commit information, flag user scores with justification for why the score is accurate or needs adjusting.
                ",
                self.project,
                self.history,
            )
        }

        fn get_history(project: &Project) -> Result<Vec<Commit>, Box<dyn Error>> {
            let repo_link = project.repo_link();
            let Some(repo_link) = repo_link else {
                return Err("No repo link".into());
            };
            let Some(project_name) = repo_link.split("/").last() else {
                return Err("No project name in repo link".into());
            };

            let output = Command::new(HISTORY_SCRIPT)
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
    }
}
