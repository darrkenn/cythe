mod docker;
mod parse_yml;
use crate::{
    docker::{build_image, cleanup_docker, run_command, start_container, stop_container},
    parse_yml::{RunStepCommand, parse_yaml, retrieve_yml},
};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use bollard::Docker;
use chrono::Local;
use dotenv::dotenv;
use hex::encode;
use hmac::{Hmac, KeyInit, Mac};
use log::{error, info, warn};
use serde::Deserialize;
use sha2::Sha256;
use std::{collections::HashMap, sync::Arc};
use subtle::ConstantTimeEq;
use tokio::fs;
use tower_http::services::ServeFile;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
struct Payload {
    pub r#ref: String,
    pub repository: Repository,
}

#[derive(Deserialize)]
struct Repository {
    pub html_url: String,
    pub full_name: String,
}
#[derive(Clone)]
struct AppState {
    allowed_repos: Arc<Vec<String>>,
    secrets: Arc<HashMap<String, String>>,
}

async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = match headers.get("X-Hub-Signature-256") {
        Some(sig) => sig.to_str().unwrap_or(""),
        None => {
            warn!("Received a request without a X-Hub-Signature");
            return (StatusCode::UNAUTHORIZED, "").into_response();
        }
    };
    let payload: Payload = match serde_json::from_slice::<Payload>(&body) {
        Ok(p) => p,
        Err(e) => {
            warn!("Received a request with invalid json: {e}");
            return (StatusCode::BAD_REQUEST, "").into_response();
        }
    };

    let repo_full_name = &payload.repository.full_name;

    if !state.allowed_repos.contains(repo_full_name) {
        warn!("Received a request from a unallowed repository");
        return (StatusCode::UNAUTHORIZED, "").into_response();
    }

    let secret = match state.secrets.get(repo_full_name) {
        Some(s) => s.trim(),
        None => {
            warn!("No secret for repo {}", repo_full_name);
            return (StatusCode::UNAUTHORIZED, "").into_response();
        }
    };

    if !verify_signature(secret, signature, &body[..]) {
        warn!("Invalid signature for {}", repo_full_name);
        return (StatusCode::UNAUTHORIZED, "").into_response();
    }

    tokio::task::spawn(async move {
        println!(
            "REF: {}, URL: {}, FULL_NAME: {}",
            payload.r#ref, payload.repository.html_url, payload.repository.full_name
        );

        let tracked_branch = match payload.r#ref.strip_prefix("refs/heads/") {
            Some(tb) => tb,
            None => {
                error!("Malformed JSON or branch isn't included");
                return;
            }
        };

        let cythe_yml = match retrieve_yml(
            payload.repository.full_name,
            tracked_branch.to_string(),
            "https://github.com".to_string(),
        )
        .await
        {
            Ok(cy) => cy,
            Err(e) => {
                error!("Error when retrieving cythe.yml: {e}");
                return;
            }
        };
        if cythe_yml.track == tracked_branch {
            let base = cythe_yml.base.to_owned();
            let (docker_file, commands) = match parse_yaml(payload.repository.html_url, cythe_yml) {
                Ok((df, commands)) => (df, commands),
                Err(e) => {
                    error!("{e}");
                    return;
                }
            };
            let _ = runner(docker_file, base, commands).await;
        }
    });

    (StatusCode::OK, "").into_response()
}

async fn runner(
    docker_file: String,
    base: String,
    commands: Vec<RunStepCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let docker = Docker::connect_with_local_defaults()?;
    let name = format!("cythe-{}", Uuid::new_v4());
    let image_name = format!("cythe-{}", base);

    match build_image(&docker, &image_name, docker_file).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    };

    let container = match start_container(&docker, &name, &image_name).await {
        Ok(c) => c,
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    };

    for step in commands {
        let (step_name, command) = (step.name, step.command);
        let command_vec: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
        println!("Container name: {}", name);
        run_command(&docker, &name, command_vec).await?;
    }

    match stop_container(&docker, &name).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    }

    match cleanup_docker(&docker, container, &name, &image_name).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    };

    Ok(())
}

fn verify_signature(secret: &str, signature: &str, body_bytes: &[u8]) -> bool {
    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(e) => {
            error!("Couldnt create HMAC: {e}");
            return false;
        }
    };
    mac.update(body_bytes);

    let result = mac.finalize();

    let code_bytes = result.into_bytes();

    let expected_signature = format!("sha256={}", encode(code_bytes));

    //Prevents timing attacks
    expected_signature
        .as_bytes()
        .ct_eq(signature.as_bytes())
        .into()
}

async fn home() -> impl IntoResponse {
    let html = fs::read_to_string("webui/index.html")
        .await
        .unwrap_or_else(|_| "<h1>Couldn't retrieve index.html</h1>".to_string());
    Html(html)
}

fn setup_logger() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .level(log::LevelFilter::Info)
        .level_for("bollard::docker", log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .chain(fern::log_file("cythe.log")?)
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}]: {}",
                Local::now().format("%d-%m-%Y %H:%M:%S"),
                record.target(),
                record.level(),
                message
            ))
        })
        .apply()?;
    Ok(())
}

async fn load_secrets() -> HashMap<String, String> {
    let mut secrets: HashMap<String, String> = HashMap::new();
    let base_dir = std::path::Path::new("/etc/cythe/secrets");

    let mut org_dirs = fs::read_dir(base_dir)
        .await
        .expect("Couldn't read signatures directory");

    while let Some(org_dir) = org_dirs.next_entry().await.unwrap() {
        let org_path = org_dir.path();
        if org_path.is_dir() {
            let org_name = org_path.file_name().unwrap().to_string_lossy();

            let mut repos = fs::read_dir(&org_path)
                .await
                .expect("Couldn't read org directory");

            while let Some(repo) = repos.next_entry().await.unwrap() {
                let repo_path = repo.path();
                if repo_path.is_file() {
                    let repo_name = repo_path.file_stem().unwrap().to_string_lossy();
                    let full_name = format!("{}/{}", org_name, repo_name);

                    let secret = fs::read_to_string(&repo_path)
                        .await
                        .expect("Couldnt read signature file");
                    secrets.insert(full_name, secret);
                }
            }
        }
    }

    secrets
}

async fn load_app_state() -> AppState {
    let data = fs::read_to_string("/etc/cythe/allowed-repos.json")
        .await
        .expect("Couldn't read allowed-repos.json");

    let allowed_repos: Vec<String> =
        serde_json::from_str(&data).expect("Couldn't parse allowed-repos.json");

    let secrets = load_secrets().await;

    AppState {
        allowed_repos: Arc::new(allowed_repos),
        secrets: Arc::new(secrets),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger().expect("Couldnt setup logger");
    info!("Staring up cythe");
    dotenv().ok();
    let app_state = load_app_state().await;

    let cythe_ci = Router::new()
        .route("/webhook", post(webhook))
        .with_state(app_state);
    let listener_ci = tokio::net::TcpListener::bind("0.0.0.0:6143").await.unwrap();

    let cythe_ui = Router::new()
        .route("/", get(home))
        .nest_service("/css", ServeFile::new("webui/static/styles.css"));
    let listener_ui = tokio::net::TcpListener::bind("0.0.0.0:3416").await.unwrap();

    let t0 = tokio::task::spawn(async move { axum::serve(listener_ci, cythe_ci).await.unwrap() });
    let t1 = tokio::task::spawn(async move { axum::serve(listener_ui, cythe_ui).await.unwrap() });
    info!("cythe CI available at 0.0.0.0:6143");
    info!("cythe UI available at 0.0.0.0:3416");
    let _ = tokio::join!(t0, t1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "9e989623257bb798879f668168fa1f3efbfce4c458a985896ef1d414fd6e733c";
    const BODY: &str = "test";

    #[tokio::test]
    async fn test_succesful_signature_verification() {
        let signature = "sha256=6b1f54cb6c6305de8a244a2e8e201ed9df89a194d83c4d290c45932c9229b3a8";
        assert!(verify_signature(SECRET, signature, BODY.as_bytes()));
    }

    #[tokio::test]
    async fn test_unsuccesful_signature_verifcation() {
        let signature = "sha256=4a2e8e201ed9df89a194d83c4d290c45932c9229b3a814244214214124214241";
        assert!(!verify_signature(SECRET, signature, BODY.as_bytes()))
    }
}
