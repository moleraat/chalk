use super::parser::Project;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;

const OWNER: &str = "moleraat";
pub const LOOKBACK_DAYS: i64 = 7;
pub const FALLBACK_COMMIT_COUNT: u32 = 10;

pub fn fetch_history(project: &Project) -> Result<Vec<Commit>, Box<dyn std::error::Error>> {
    // parse repo name from toml link
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

    // get commit info across branches
    let branches: Vec<GhBranch> = gh_api(&format!("repos/{OWNER}/{repo}/branches"))?;
    let mut commits = Vec::<Commit>::new();
    for branch in branches {
        let summaries: Vec<GhCommitSummary> = gh_api(&format!(
            "repos/{OWNER}/{repo}/commits?sha={}&since={since}",
            branch.name
        ))?;
        for summary in summaries {
            commits.push(fetch_commit(repo, &branch.name, &summary.sha)?);
        }
    }

    // if inactive project, grab old commits
    if commits.is_empty() {
        let summaries: Vec<GhCommitSummary> = gh_api(&format!(
            "repos/{OWNER}/{repo}/commits?per_page={FALLBACK_COMMIT_COUNT}"
        ))?;
        for summary in summaries {
            commits.push(fetch_commit(repo, "default", &summary.sha)?);
        }
    }

    Ok(commits)
}

fn fetch_commit(
    repo: &str,
    branch: &str,
    sha: &str,
) -> Result<Commit, Box<dyn std::error::Error>> {
    let detail: GhCommitDetail = gh_api(&format!("repos/{OWNER}/{repo}/commits/{sha}"))?;
    Ok(Commit {
        hash: detail.sha,
        branch: branch.to_string(),
        subject: detail.commit.message,
        date: detail.commit.author.date,
        link: detail.html_url,
        delta: Delta {
            additions: detail.stats.additions,
            deletions: detail.stats.deletions,
        },
    })
}

pub fn create_issue(title: &str, body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new("gh")
        .args(["issue", "create", "--title", title, "--body", body])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "gh issue create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn gh_api<T: DeserializeOwned>(path: &str) -> Result<T, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("gh")
        .args(["api", path])
        .output()?;
    let body = String::from_utf8(output.stdout)?;
    Ok(serde_json::from_str(&body)?)
}

#[derive(Deserialize, Debug)]
pub struct Commit {
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
