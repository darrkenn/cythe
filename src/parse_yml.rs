use log::error;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Step {
    pub name: String,
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
    NonSuccessStatus(u16),
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
            YamlError::NonSuccessStatus(s) => write!(f, "Non success status: {}", s),
            YamlError::CantParse(s) => write!(f, "{}", s),
            YamlError::NotReadable(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for YamlError {
    fn description(&self) -> &str {
        match self {
            YamlError::NotAUseCommand(s) => s,
            YamlError::NotFound => "cythe.yml not found",
            YamlError::NonSuccessStatus(_) => "non successfull status",
            YamlError::CantParse(s) => s,
            YamlError::NotReadable(s) => s,
        }
    }
}

pub fn create_docker_file(git_url: String, cythe_yml: CytheYML) -> Result<String, YamlError> {
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
    //Git Hosting Platform URL
    ghp_url: String,
) -> Result<CytheYML, YamlError> {
    let url = format!(
        "{}/{}/{}/cythe.yml",
        ghp_url, repo_full_name, tracked_branch
    );
    let response = match Client::new()
        .get(&url)
        .header("Cache-Control", "no-cache")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("Network error while retrieving cythe.yml: {e}");
            println!("{e}");
            return Err(YamlError::NotFound);
        }
    };
    if response.status() != 200 {
        error!(
            "Github responded with a non 200 status: {}",
            response.status()
        );
        return Err(YamlError::NonSuccessStatus(response.status().as_u16()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_retrieve_parser_successful() {
        let mut server = mockito::Server::new_async().await;

        let yaml_content = r#"
base: rust:slim-trixie
track: main

steps:
  - name: Update
    run: apt update && apt-get install git -y

  - name: Checkout repo
    use: cythe-checkout

  - name: Build
    run: cargo build --release

  - name: What
    run: ls -la ./target/release/ | grep cythe

  - name: Run
    entrypoint: ./target/release/cythe-test
"#;

        let _ = server
            .mock("GET", "/user/repo/main/cythe.yml")
            .with_status(200)
            .with_body(yaml_content)
            .create_async()
            .await;
        let server_url = server.url();

        let result = retrieve_yml("user/repo".to_string(), "main".to_string(), server_url)
            .await
            .expect("Couldn't retrieve yml");
        assert_eq!(result.base, "rust:slim-trixie");
        assert_eq!(result.track, "main");

        assert_eq!(result.steps.len(), 5);
        assert_eq!(result.steps[0].name, "Update");
        assert_eq!(result.steps[1].name, "Checkout repo");
        assert_eq!(result.steps[2].name, "Build");
        assert_eq!(result.steps[3].name, "What");
        assert_eq!(result.steps[4].name, "Run");
    }

    #[tokio::test]
    async fn test_retrieve_parser_not_found() {
        let bad_url = "http://127.0.0.1:0".to_string();
        let result = retrieve_yml("user/repo".to_string(), "main".to_string(), bad_url).await;

        match result {
            Err(YamlError::NotFound) => {}
            _ => panic!("Expected error NotFound"),
        }
    }

    #[tokio::test]
    async fn test_retrieve_parser_non_success() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/user/repo/main/cythe.yml")
            .with_status(404)
            .with_body("Not found")
            .create_async()
            .await;
        let server_url = server.url();

        let result = retrieve_yml("user/repo".to_string(), "main".to_string(), server_url).await;

        match result {
            Err(YamlError::NonSuccessStatus(code)) => assert_eq!(code, 404),
            _ => panic!("Expected error NonSuccessStatus"),
        }
    }

    #[tokio::test]
    async fn test_retrieve_parser_invalid_yaml() {
        let mut server = mockito::Server::new_async().await;

        let invalid_yaml = "what: the _: hell is this:_# yaml";

        let _ = server
            .mock("GET", "/user/repo/main/cythe.yml")
            .with_status(200)
            .with_body(invalid_yaml)
            .create_async()
            .await;
        let server_url = server.url();

        let result = retrieve_yml("user/repo".to_string(), "main".to_string(), server_url).await;

        match result {
            Err(YamlError::CantParse(_)) => {}
            _ => panic!("Expected error CantParse"),
        }
    }

    #[tokio::test]
    async fn test_create_docker_file_successful() {
        let git_url = "https://github.com/darrkenn/cythe".to_string();
        let cythe_yml = CytheYML {
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
            ],
        };

        let result = create_docker_file(git_url, cythe_yml).expect("Couldnt create docker file");

        assert!(result.contains("FROM rust:slim-trixie"));
        assert!(result.contains("WORKDIR /app"));
        assert!(result.contains("RUN apt update && apt-get install git -y"));
        assert!(result.contains("RUN git clone --depth=1 https://github.com/darrkenn/cythe ."));
        assert!(result.contains("RUN cargo build --release"));
        assert!(result.contains(r#"ENTRYPOINT ["sh", "-c", "./target/release/cythe"]"#));
    }

    #[tokio::test]
    async fn test_create_docker_file_invalid_use_command() {
        let git_url = "https://github.com/darrkenn/cythe".to_string();
        let cythe_yml = CytheYML {
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
