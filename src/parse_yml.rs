use std::path::Path;

use git2::{BranchType, Repository};
use log::error;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct Step {
    pub name: String,
    pub run: Option<String>,
    pub r#use: Option<String>,
    pub entrypoint: Option<String>,
    pub cmd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CytheYAML {
    pub base: String,
    pub track: String,
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

pub fn create_docker_file(git_url: String, cythe_yml: CytheYAML) -> Result<String, YamlError> {
    let mut docker_file = format!("FROM {}\nWORKDIR /app\n", cythe_yml.base);

    for step in cythe_yml.steps {
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
            docker_file.push_str(&format!(
                "ENTRYPOINT [\"sh\", \"-c\", \"{}\"]\n",
                entrypoint
            ))
        }
        if let Some(cmd) = step.cmd {
            let parts = cmd.split_whitespace().collect::<Vec<_>>();
            let cmd = parts
                .iter()
                .map(|p| {
                    if p.starts_with('"') && p.ends_with('"') {
                        p.to_string()
                    } else {
                        format!("\"{}\"", p)
                    }
                })
                .collect::<Vec<String>>()
                .join(", ");
            docker_file.push_str(&format!("CMD [{}]\n", cmd))
        }
    }

    Ok(docker_file)
}

pub async fn retrieve_yml(
    repo_full_name: String,
    tracked_branch: String,
    //Git Hosting Platform URL
    ghp_url: String,
) -> Result<CytheYAML, YamlError> {
    let repo_url = format!("{}/{}.git", ghp_url, repo_full_name);
    let temp_path = format!("/tmp/cythe-{}", Uuid::new_v4());

    let repo = Repository::clone(&repo_url, &temp_path).map_err(|e| {
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
    async fn test_create_docker_file_successful() {
        let git_url = "https://github.com/darrkenn/cythe".to_string();
        let cythe_yml = CytheYAML {
            base: String::from("rust:slim-trixie"),
            track: String::from("main"),
            steps: vec![
                Step {
                    name: String::from("Update"),
                    run: Some(String::from("apt update && apt-get install git -y")),
                    r#use: None,
                    entrypoint: None,
                    cmd: None,
                },
                Step {
                    name: String::from("Checkout repo"),
                    run: None,
                    r#use: Some(String::from("cythe-checkout")),
                    entrypoint: None,
                    cmd: None,
                },
                Step {
                    name: String::from("Build"),
                    run: Some(String::from("cargo build --release")),
                    r#use: None,
                    entrypoint: None,
                    cmd: None,
                },
                Step {
                    name: String::from("Run"),
                    run: None,
                    r#use: None,
                    entrypoint: Some(String::from("./target/release/cythe")),
                    cmd: None,
                },
                Step {
                    name: String::from("test"),
                    run: None,
                    r#use: None,
                    entrypoint: None,
                    cmd: Some(String::from("echo \"hi\"")),
                },
            ],
        };

        let result = create_docker_file(git_url, cythe_yml).expect("Couldnt create docker file");

        assert!(result.contains("FROM rust:slim-trixie"));
        assert!(result.contains("WORKDIR /app"));
        assert!(result.contains("RUN apt update && apt-get install git -y"));
        assert!(result.contains("RUN git clone --depth=1 https://github.com/darrkenn/cythe ."));
        assert!(result.contains("RUN cargo build --release"));
        assert!(result.contains(r#"ENTRYPOINT ["sh", "-c", "./target/release/cythe"]"#));
        assert!(result.contains(r#"CMD ["echo", "hi"]"#));
    }

    #[tokio::test]
    async fn test_create_docker_file_invalid_use_command() {
        let git_url = "https://github.com/darrkenn/cythe".to_string();
        let cythe_yml = CytheYAML {
            base: String::from("rust:slim-trixie"),
            track: String::from("main"),
            steps: vec![Step {
                name: String::from("Update"),
                run: None,
                r#use: Some(String::from("invalid")),
                entrypoint: None,
                cmd: None,
            }],
        };

        let result = create_docker_file(git_url, cythe_yml);
        match result {
            Err(YamlError::NotAUseCommand(cmd)) => assert_eq!(cmd, "invalid"),
            _ => panic!("Expected error NotAUseCommand"),
        }
    }
}
