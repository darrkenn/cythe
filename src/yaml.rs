use std::{collections::HashMap, path::Path};

use git2::{BranchType, Repository};
use log::error;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct Step {
    pub name: String,
    pub run: Option<String>,
    pub r#use: Option<String>,
}

pub struct RunStepCommand {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Deserialize)]
pub struct CytheYAML {
    pub image: String,
    pub steps: Vec<Step>,
}

#[derive(Debug)]
pub enum GitError {
    CloneFailed(String),
    BranchError(String),
    CommitError(String),
    TreeError(String),
    BlobError(String),
    ObjectError(String),
    Utf8Error,
}

#[derive(Debug)]
pub enum YamlError {
    NotAUseCommand(String),
    CantParse(String),
    Git(GitError),
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YamlError::NotAUseCommand(s) => {
                write!(f, "The following is not an internal command: {}", s)
            }
            YamlError::CantParse(s) => write!(f, "{}", s),
            YamlError::Git(s) => write!(f, "Git Error {}", s),
        }
    }
}

impl ::std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::CloneFailed(s) => write!(f, "Clone failed: {}", s),
            GitError::BranchError(s) => write!(f, "Branch Error: {}", s),
            GitError::CommitError(s) => write!(f, "Commit Error: {}", s),
            GitError::TreeError(s) => write!(f, "Tree Error: {}", s),
            GitError::BlobError(s) => write!(f, "Blob Error: {}", s),
            GitError::ObjectError(s) => write!(f, "Object Error: {}", s),
            GitError::Utf8Error => write!(f, "Utf8 Error"),
        }
    }
}

pub fn parse_yaml(
    git_url: &String,
    cythe_yml: CytheYAML,
    repo_secrets: Option<HashMap<String, String>>,
) -> Result<(String, Vec<RunStepCommand>), YamlError> {
    let mut steps: Vec<RunStepCommand> = Vec::new();

    for step in cythe_yml.steps {
        if let Some(internal_command) = step.r#use {
            match internal_command.as_str() {
                "cythe-checkout" => {
                    steps.push(RunStepCommand {
                        name: step.name,
                        command: format!("git clone --depth=1 {} .", git_url),
                    });
                    continue;
                }
                s => return Err(YamlError::NotAUseCommand(s.to_string())),
            }
        }
        if let Some(run) = step.run {
            let command = if let Some(repo_secrets) = &repo_secrets {
                replace_secrets(run, repo_secrets)
            } else {
                run
            };
            steps.push(RunStepCommand {
                name: step.name,
                command,
            })
        }
    }
    Ok((cythe_yml.image, steps))
}

fn replace_secrets(command: String, secrets: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next();
            let mut key = String::new();

            while let Some(&c) = chars.peek() {
                if c == '}' {
                    chars.next();
                    break;
                }
                key.push(chars.next().unwrap());
            }

            if let Some(value) = secrets.get(&key) {
                result.push_str(value);
            } else {
                result.push_str(&format!("${{{}}}", key));
            }
        } else {
            result.push(ch);
        }
    }
    result
}

pub async fn retrieve_yaml(tracked_branch: String, git_url: &str) -> Result<CytheYAML, YamlError> {
    let temp_path = format!("/tmp/cythe-{}", Uuid::new_v4());

    let repo = Repository::clone(git_url, &temp_path).map_err(|e| {
        error!("{e}");
        YamlError::Git(GitError::CloneFailed(e.to_string()))
    })?;

    let reference = repo
        .find_branch(&tracked_branch, BranchType::Local)
        .map_err(|e| {
            error!("{e}");
            YamlError::Git(GitError::BranchError(e.to_string()))
        })?
        .into_reference();

    let commit = reference.peel_to_commit().map_err(|e| {
        error!("{e}");
        YamlError::Git(GitError::CommitError(e.to_string()))
    })?;

    let tree = commit.tree().map_err(|e| {
        error!("{e}");
        YamlError::Git(GitError::TreeError(e.to_string()))
    })?;

    let tree_entry = tree.get_path(Path::new("cythe.yml")).map_err(|e| {
        error!("{e}");
        YamlError::Git(GitError::BlobError(e.to_string()))
    })?;

    let object = tree_entry.to_object(&repo).map_err(|e| {
        error!("{e}");
        YamlError::Git(GitError::ObjectError(e.to_string()))
    })?;

    let blob = object.as_blob().ok_or_else(|| {
        error!("cythe.yml is not a file");
        YamlError::Git(GitError::BlobError("cythe.yml is not a file".to_string()))
    })?;

    let content = std::str::from_utf8(blob.content()).map_err(|e| {
        error!("{e}");
        YamlError::Git(GitError::Utf8Error)
    })?;

    match serde_yaml::from_str::<CytheYAML>(content) {
        Ok(c) => Ok(c),
        Err(e) => {
            error!("Can't parse cythe.yml: {e}");
            Err(YamlError::CantParse(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parse_yaml_successful() {
        let git_url = "https://github.com/darrkenn/cythe".to_string();

        let cythe_yaml = CytheYAML {
            image: String::from("rust:slim-trixie"),
            steps: vec![
                Step {
                    name: String::from("Checkout"),
                    run: None,
                    r#use: Some(String::from("cythe-checkout")),
                },
                Step {
                    name: String::from("Test"),
                    run: Some(String::from("cargo test --verbose")),
                    r#use: None,
                },
            ],
        };

        let (image, commands) =
            parse_yaml(&git_url, cythe_yaml, None).expect("Couldnt create docker file");
        let command_one: &RunStepCommand = &commands[0];
        let command_two: &RunStepCommand = &commands[1];

        assert!(image.contains("rust:slim-trixie"));
        assert!(command_one.name.contains("Checkout"));
        assert!(
            command_one
                .command
                .contains("git clone --depth=1 https://github.com/darrkenn/cythe .")
        );
        assert!(command_two.name.contains("Test"));
        assert!(command_two.command.contains("cargo test --verbose"));
    }

    #[tokio::test]
    async fn parse_yaml_invalid_use_command() {
        let git_url = "https://github.com/darrkenn/cythe".to_string();
        let cythe_yaml = CytheYAML {
            image: String::from("rust:slim-trixie"),
            steps: vec![Step {
                name: String::from("invalid"),
                run: None,
                r#use: Some(String::from("invalid")),
            }],
        };

        let result = parse_yaml(&git_url, cythe_yaml, None);
        match result {
            Err(YamlError::NotAUseCommand(cmd)) => assert_eq!(cmd, "invalid"),
            _ => panic!("Expected error NotAUseCommand"),
        }
    }

    #[tokio::test]
    async fn replace_secrets_successful() {
        let mut secrets: HashMap<String, String> = HashMap::new();
        secrets.insert(String::from("HELLO"), String::from("hi"));
        let command = String::from("echo ${HELLO}");
        let replaced_command = replace_secrets(command, &secrets);
        assert_eq!(replaced_command, "echo hi");
    }

    #[tokio::test]
    async fn replace_secrets_no_matching_secret() {
        let secrets: HashMap<String, String> = HashMap::new();
        let command = String::from("echo ${HELLO}");
        let replaced_command = replace_secrets(command, &secrets);
        assert_eq!(replaced_command, "echo ${HELLO}");
    }
}
