use std::{collections::HashMap, sync::Arc};

use serde::Deserialize;
use tokio::sync::Semaphore;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub cache_images: bool,
    pub max_active_runners: u8,
    pub log_level: String,
}

#[derive(Clone)]
pub struct AppState {
    pub allowed_repos: Arc<Vec<String>>,
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

pub fn load_app_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let data = std::fs::read_to_string("/etc/cythe/allowed-repos.json")?;

    let allowed_repos: Vec<String> =
        serde_json::from_str(&data).expect("Couldn't parse allowed-repos.json");

    let secrets = match load_secrets() {
        Ok(s) => s,
        Err(e) => {
            return Err(e);
        }
    };

    let config_string = std::fs::read_to_string("/etc/cythe/config.toml")?;
    let config: Config = toml::from_str(&config_string).expect("Couldnt parse config.toml");

    Ok(AppState {
        allowed_repos: Arc::new(allowed_repos),
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
}
