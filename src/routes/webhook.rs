#[cfg(debug_assertions)]
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
    pub html_url: String,
    pub full_name: String,
}
#[derive(Deserialize)]
struct Payload {
    pub r#ref: String,
    pub repository: Repository,
}

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
        let tracked_branch = match payload.r#ref.strip_prefix("refs/heads/") {
            Some(tb) => tb,
            None => {
                error!("Malformed JSON or branch isn't included");
                return;
            }
        };
        let repo_full_name = payload.repository.full_name;

        let cythe_yml = match retrieve_yaml(
            &repo_full_name,
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
            let (image, commands) = match parse_yaml(payload.repository.html_url, cythe_yml) {
                Ok((image_type, commands)) => (image_type, commands),
                Err(e) => {
                    error!("{e}");
                    return;
                }
            };
            let cache_images = state.config.cache_images;
            let max_runners = state.config.max_active_runners;
            let continue_on_fail = state.config.continue_on_fail;
            info!("Trying to start runner for: {}", repo_full_name);
            let _permit = state.active_runners.clone().acquire_owned().await.unwrap();

            info!(
                "Runner started for {}. Active runners: {}/{}",
                &repo_full_name,
                max_runners - state.active_runners.available_permits() as u8,
                max_runners
            );

            match runner(image, commands, cache_images, continue_on_fail).await {
                Ok((logs, failed)) => {
                    info!("Runner for {repo_full_name} completed successfully");
                    let date = chrono::Local::now().format("%d-%m-%Y %H:%M:%S").to_string();
                    let pipeline_entry =
                        PipelineEntry::new(repo_full_name.clone(), logs, failed, date);
                    match database::create_pipeline_entry(pipeline_entry) {
                        Ok(_) => {
                            info!(
                                "Successfully created database log entry for: {}",
                                repo_full_name
                            );
                        }
                        Err(e) => {
                            error!("Couldn't create database log entry for: {repo_full_name}. {e}");
                        }
                    };
                }
                Err(e) => error!("Runner failed for {}: {e}", repo_full_name),
            }

            //Fixes inaccurate count of active runners
            drop(_permit);

            info!(
                "Runner for {} finished. Active runners: {}/{}",
                repo_full_name,
                max_runners - state.active_runners.available_permits() as u8,
                max_runners
            );
        } else {
            info!("Not tracking the branch: {}", tracked_branch);
        }
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
}

#[cfg(debug_assertions)]
pub async fn webhook_debug(
    State(state): State<AppState>,
    repo_query: Query<DebugWebhookQuery>,
) -> impl IntoResponse {
    info!("Received request to /webhook_debug");

    let repo_name = repo_query.name.clone();
    let tracked_branch = repo_query.tracked_branch.clone();

    tokio::task::spawn(async move {
        let cythe_yml = match retrieve_yaml(
            &repo_name,
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

        let git_url = format!("https://github.com/{}", repo_name);

        let (image, commands) = match parse_yaml(git_url, cythe_yml) {
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
