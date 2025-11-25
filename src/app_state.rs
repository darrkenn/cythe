use std::{collections::HashMap, fs, io::Write, sync::Arc};

use serde::Deserialize;
use tokio::sync::Semaphore;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub cache_images: bool,
    pub max_active_runners: u8,
    pub log_level: String,
    pub continue_on_fail: bool,
}

#[derive(Deserialize)]
pub struct Repo {
    pub name: String,
    pub tracked_branch: String,
    pub url: String,
    pub secrets: Option<HashMap<String, String>>,
}

pub struct RepoInfo {
    pub tracked_branch: String,
    pub url: String,
    pub secrets: Option<HashMap<String, String>>,
}

#[derive(serde::Deserialize)]
struct Repos {
    repo: Vec<Repo>,
}

#[derive(Clone)]
pub struct AppState {
    pub repos: Arc<HashMap<String, RepoInfo>>,
    pub secrets: Arc<HashMap<String, String>>,
    pub active_runners: Arc<Semaphore>,
    pub config: Arc<Config>,
}
fn load_secrets() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut secrets: HashMap<String, String> = HashMap::new();
    let base_dir = std::path::Path::new("/etc/cythe/secrets");

    let org_dirs = std::fs::read_dir(base_dir)?;
    for org_dir in org_dirs {
        let org_dir = org_dir?;
        let org_path = org_dir.path();

        if org_path.is_dir() {
            let org_name = org_path.file_name().unwrap().to_string_lossy();
            let repos = std::fs::read_dir(&org_path)?;

            for repo in repos {
                let repo = repo?;
                let repo_path = repo.path();

                if repo_path.is_file() {
                    let repo_name = repo_path.file_stem().unwrap().to_string_lossy();
                    let full_name = format!("{}/{}", org_name, repo_name);
                    let secret = std::fs::read_to_string(&repo_path)?;
                    secrets.insert(full_name, secret);
                }
            }
        }
    }

    Ok(secrets)
}

fn create_directories_files() -> Result<(), Box<dyn std::error::Error>> {
    let dirs: Vec<&str> = vec![
        "/etc/cythe",
        "/var/log/cythe",
        "/var/lib/cythe",
        "/etc/cythe/secrets",
    ];
    let files: Vec<&str> = vec![
        "/etc/cythe/config.toml",
        "/etc/cythe/repos.toml",
        "/var/log/cythe/cythe.log",
        "/var/lib/cythe/cythe.db",
    ];

    for dir in dirs {
        fs::create_dir_all(dir)?;
    }
    for file in files {
        if !std::path::Path::new(file).exists() {
            let mut created_file = fs::File::create(file)?;
            let content = match file {
                "/etc/cythe/config.toml" => {
                    "cache_images = true\nmax_active_runners = 2\nlog_level = \"info\"\ncontinue_on_fail = false"
                }
                "/etc/cythe/repos.toml" => {
                    "[[repo]]\nname = \"org/name\"\ntracked_branch = \"main\"\nurl = \"https://github.com/org/name\"\n[repo.secrets]\nSECRET = \"secret\""
                }
                _ => continue,
            };
            created_file.write_all(content.as_bytes())?;
        }
    }
    Ok(())
}

pub fn load_app_state() -> Result<AppState, Box<dyn std::error::Error>> {
    create_directories_files()?;
    let repos_data = std::fs::read_to_string("/etc/cythe/repos.toml")?;
    let repos: Repos = toml::from_str(&repos_data)?;
    let secrets = load_secrets()?;

    let config_data = std::fs::read_to_string("/etc/cythe/config.toml")?;
    let config: Config = toml::from_str(&config_data)?;

    let mut repos_hashmap: HashMap<String, RepoInfo> = HashMap::new();

    for repo in repos.repo {
        let repo_info = RepoInfo {
            tracked_branch: repo.tracked_branch,
            url: repo.url,
            secrets: repo.secrets,
        };
        repos_hashmap.insert(repo.name, repo_info);
    }

    Ok(AppState {
        repos: Arc::new(repos_hashmap),
        secrets: Arc::new(secrets),
        active_runners: Arc::new(Semaphore::new(config.max_active_runners as usize)),
        config: Arc::new(config),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn load_secrets_successful() {
        match load_secrets() {
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }

    #[tokio::test]
    async fn test_create_directories_files() {
        match create_directories_files() {
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
}
