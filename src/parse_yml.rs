use log::error;
use reqwest::{Client, get};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Step {
    pub run: Option<String>,
    pub r#use: Option<String>,
    pub entrypoint: Option<String>,
    pub cmd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CytheYML {
    pub base: String,
    pub track: String,
    pub steps: Vec<Step>,
}

#[derive(Debug)]
pub enum YamlError {
    NotAUseCommand(String),
    NotFound,
    CantParse(String),
    NotReadable(String),
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YamlError::NotAUseCommand(s) => {
                write!(f, "The following is not an internal command: {}", s)
            }
            YamlError::NotFound => write!(f, "cythe.yml not found"),
            YamlError::CantParse(s) => write!(f, "{}", s),
            YamlError::NotReadable(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for YamlError {
    fn description(&self) -> &str {
        match self {
            YamlError::NotAUseCommand(s) => s,
            YamlError::NotFound => "",
            YamlError::CantParse(s) => s,
            YamlError::NotReadable(s) => s,
        }
    }
}

pub fn create_docker_file(
    git_url: String,
    tracked_branch: String,
    config: CytheYML,
) -> Result<String, YamlError> {
    let mut docker_file = format!("FROM {}\nWORKDIR /app\n", config.base);

    for step in config.steps {
        if let Some(internal_command) = step.r#use {
            match internal_command.as_str() {
                "cythe-checkout" => {
                    docker_file.push_str(&format!("RUN git clone --depth=1 {} .\n", git_url));
                }
                s => return Err(YamlError::NotAUseCommand(s.to_string())),
            }
        }
        if let Some(run) = step.run {
            docker_file.push_str(&format!("RUN {}\n", run));
        }
        if let Some(entrypoint) = step.entrypoint {
            docker_file.push_str(&format!(r#"ENTRYPOINT ["sh", "-c", "{}"]"#, entrypoint))
        }
        if let Some(cmd) = step.cmd {
            docker_file.push_str(&format!("CMD ['{}']", cmd))
        }
    }

    Ok(docker_file)
}

pub async fn retrieve_yml(
    repo_full_name: String,
    tracked_branch: String,
) -> Result<CytheYML, YamlError> {
    let raw_url = format!(
        "https://raw.githubusercontent.com/{}/{}/cythe.yml",
        repo_full_name, tracked_branch
    );
    let response = match Client::new()
        .get(&raw_url)
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("Network error while retrieving cythe.yml: {e}");
            return Err(YamlError::NotFound);
        }
    };
    if response.status() != 200 {
        error!(
            "Github responded with a non 200 status: {}",
            response.status()
        );
        return Err(YamlError::NotFound);
    };

    match response.text().await {
        Ok(t) => match serde_yaml::from_str::<CytheYML>(&t) {
            Ok(c) => Ok(c),
            Err(e) => {
                error!("Cant parse cythe.yml: {e}");
                Err(YamlError::CantParse(e.to_string()))
            }
        },
        Err(e) => {
            error!("Error reading cythe.yml: {e}");
            Err(YamlError::NotReadable(e.to_string()))
        }
    }
}
