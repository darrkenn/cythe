use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Step {
    pub run: Option<String>,
    pub r#use: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub base: String,
    pub steps: Vec<Step>,
}

#[derive(Debug)]
pub enum YamlError {
    NotAUseCommand(String),
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YamlError::NotAUseCommand(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for YamlError {
    fn description(&self) -> &str {
        match self {
            YamlError::NotAUseCommand(s) => s,
        }
    }
}

pub fn create_docker_config(
    git_url: String,
    github_user: String,
    config: Config,
) -> Result<String, YamlError> {
    let mut docker_file = format!("FROM {}\nWORKDIR /app\n", config.base);

    for step in config.steps {
        if let Some(internal_command) = step.r#use {
            match internal_command.as_str() {
                "ciythe-checkout" => {
                    docker_file.push_str(&format!("RUN git clone --depth=1 {} .\n", git_url));
                }
                s => return Err(YamlError::NotAUseCommand(s.to_string())),
            }
        }
        if let Some(cmd) = step.run {
            docker_file.push_str(&format!("RUN {}\n", cmd));
        }
    }

    Ok(docker_file)
}
