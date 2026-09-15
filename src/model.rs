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

pub mod context;
pub mod request;
