use axum::extract::Query;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use hex::encode;
use hmac::{Hmac, KeyInit, Mac};
use log::{error, info, warn};
use serde::Deserialize;
use sha2::Sha256;
use subtle::ConstantTimeEq;

#[derive(Deserialize)]
struct Repository {
    pub full_name: String,
}
#[derive(Deserialize)]
struct Payload {
    pub r#ref: String,
    pub repository: Repository,
}

use crate::parse_yml::CytheYAML;
use crate::{
    app_state::AppState,
    database::{self, PipelineEntry},
    parse_yml::{parse_yaml, retrieve_yaml},
    runner::runner,
};

type HmacSha256 = Hmac<Sha256>;

pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    info!("Post request to /webhook received");
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
    let repo_name = payload.repository.full_name.clone();

    if !state.repos.contains_key(&repo_name) {
        return (StatusCode::UNAUTHORIZED, "").into_response();
    }

    let remote_branch = payload.r#ref.strip_prefix("refs/heads/").unwrap();
    let local_branch = state
        .repos
        .get(&repo_name)
        .map(|ri| ri.tracked_branch.clone())
        .unwrap();

    if local_branch != remote_branch {
        warn!(
            "Remote branch {} does not match local branch {}",
            remote_branch, local_branch
        );
        return (StatusCode::UNAUTHORIZED, "").into_response();
    } else {
        println!("Valid branch: {}, Remote: {}", local_branch, remote_branch)
    }

    let secret = match state.secrets.get(&repo_name) {
        Some(s) => s.trim(),
        None => {
            warn!("No secret for repo {}", repo_name);
            return (StatusCode::UNAUTHORIZED, "").into_response();
        }
    };
    if !verify_signature(secret, signature, &body[..]) {
        warn!("Invalid signature for {}", repo_name);
        return (StatusCode::UNAUTHORIZED, "").into_response();
    }

    let git_url = state.repos.get(&repo_name).unwrap().url.clone();
    let repo_secrets = state.repos.get(&repo_name).unwrap().secrets.clone();

    tokio::task::spawn(async move {
        let cythe_yml = match retrieve_yaml(local_branch, &git_url).await {
            Ok(cy) => cy,
            Err(e) => {
                error!("Error when retrieving cythe.yml: {e}");
                return;
            }
        };
        let (image, commands) = match parse_yaml(&git_url, cythe_yml, repo_secrets) {
            Ok((image_type, commands)) => (image_type, commands),
            Err(e) => {
                error!("{e}");
                return;
            }
        };
        let cache_images = state.config.cache_images;
        let max_runners = state.config.max_active_runners;
        let continue_on_fail = state.config.continue_on_fail;
        info!("Trying to start runner for: {}", repo_name);
        let _permit = state.active_runners.clone().acquire_owned().await.unwrap();

        info!(
            "Runner started for {}. Active runners: {}/{}",
            &repo_name,
            max_runners - state.active_runners.available_permits() as u8,
            max_runners
        );

        match runner(image, commands, cache_images, continue_on_fail).await {
            Ok((logs, failed)) => {
                info!("Runner for {repo_name} completed successfully");
                let date = chrono::Local::now().format("%d-%m-%Y %H:%M:%S").to_string();
                let pipeline_entry = PipelineEntry::new(repo_name.clone(), logs, failed, date);
                match database::create_pipeline_entry(pipeline_entry) {
                    Ok(_) => {
                        info!("Successfully created database log entry for: {}", repo_name);
                    }
                    Err(e) => {
                        error!("Couldn't create database log entry for: {repo_name}. {e}");
                    }
                };
            }
            Err(e) => error!("Runner failed for {}: {e}", repo_name),
        }

        //Fixes inaccurate count of active runners
        drop(_permit);

        info!(
            "Runner for {} finished. Active runners: {}/{}",
            repo_name,
            max_runners - state.active_runners.available_permits() as u8,
            max_runners
        );
    });

    (StatusCode::OK, "").into_response()
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

#[derive(Deserialize)]
pub struct DebugWebhookQuery {
    name: String,
    tracked_branch: String,
    git_url: String,
    cythe_yml_location: Option<String>,
}

pub async fn webhook_debug(
    State(state): State<AppState>,
    repo_query: Query<DebugWebhookQuery>,
) -> impl IntoResponse {
    info!("Received request to /webhook_debug");

    let repo_name = repo_query.name.clone();
    let remote_branch = repo_query.tracked_branch.clone();
    let git_url = repo_query.git_url.clone();
    let cythe_yml_location = repo_query.cythe_yml_location.clone();
    let repo_secrets = state.repos.get(&repo_name).unwrap().secrets.clone();

    let local_branch = match state.repos.get(&repo_name) {
        Some(ri) => ri.tracked_branch.clone(),
        None => {
            warn!("Repository {} not found", repo_name);
            warn!(
                "Available repos: {:?}",
                state.repos.keys().collect::<Vec<_>>()
            );
            return (StatusCode::UNAUTHORIZED, "").into_response();
        }
    };
    if local_branch != remote_branch {
        warn!(
            "Remote branch {} does not match local branch {}",
            remote_branch, local_branch
        );
        return (StatusCode::UNAUTHORIZED, "").into_response();
    } else {
        println!("Valid branch: {}, Remote: {}", local_branch, remote_branch)
    }

    tokio::task::spawn(async move {
        let cythe_yml = if let Some(cythe_yml_location) = cythe_yml_location {
            let content = std::fs::read_to_string(cythe_yml_location).unwrap();
            match serde_yaml::from_str::<CytheYAML>(&content) {
                Ok(c) => c,
                Err(e) => {
                    error!("Can't parse cythe.yml: {e}");
                    return;
                }
            }
        } else {
            match retrieve_yaml(remote_branch, &git_url).await {
                Ok(cy) => cy,
                Err(e) => {
                    error!("Error when retrieving cythe.yml: {e}");
                    return;
                }
            }
        };

        let (image, commands) = match parse_yaml(&git_url, cythe_yml, repo_secrets) {
            Ok((image_type, commands)) => (image_type, commands),
            Err(e) => {
                error!("{e}");
                return;
            }
        };

        let cache_images = state.config.cache_images;
        let max_runners = state.config.max_active_runners;
        let continue_on_fail = state.config.continue_on_fail;
        info!("Trying to start runner for: {}", repo_name);

        let _permit = state.active_runners.clone().acquire_owned().await.unwrap();

        info!(
            "Runner started for {}. Active runners: {}/{}",
            &repo_name,
            max_runners - state.active_runners.available_permits() as u8,
            max_runners
        );

        match runner(image, commands, cache_images, continue_on_fail).await {
            Ok((logs, failed)) => {
                info!("Runner for {repo_name} completed successfully");
                let date = chrono::Local::now().format("%d-%m-%Y %H:%M:%S").to_string();
                let pipeline_entry = PipelineEntry::new(repo_name.clone(), logs, failed, date);
                match database::create_pipeline_entry(pipeline_entry) {
                    Ok(_) => {
                        info!("Successfully created database log entry for: {}", repo_name);
                    }
                    Err(e) => {
                        error!("Couldn't create database log entry for: {repo_name}. {e}");
                    }
                };
            }
            Err(e) => error!("Runner failed for {}: {e}", repo_name),
        }

        //Fixes inaccurate count of active runners
        drop(_permit);

        info!(
            "Runner for {} finished. Active runners: {}/{}",
            repo_name,
            max_runners - state.active_runners.available_permits() as u8,
            max_runners
        );
    });

    (StatusCode::OK, "").into_response()
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
