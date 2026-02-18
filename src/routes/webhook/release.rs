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

use crate::{
    app_state::AppState,
    database::{self, PipelineEntry},
    runner::runner,
    yaml::{parse_yaml, retrieve_yaml},
};
use crate::{build_telefy_message, invalid_request};

type HmacSha256 = Hmac<Sha256>;

pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = match headers.get("X-Hub-Signature-256") {
        Some(sig) => sig.to_str().unwrap_or(""),
        None => {
            invalid_request!(
                StatusCode::UNAUTHORIZED,
                "Received a request without a X-Hub-Signature"
            );
        }
    };
    let payload: Payload = match serde_json::from_slice::<Payload>(&body) {
        Ok(p) => p,
        Err(_) => {
            invalid_request!(
                StatusCode::BAD_REQUEST,
                "Received a request with invalid json"
            );
        }
    };
    let repo_name = payload.repository.full_name.clone();

    // Checks if payload repo is allowed
    if !state.repos.contains_key(&repo_name) {
        invalid_request!(
            StatusCode::UNAUTHORIZED,
            format!("Invalid repo: {}", &repo_name)
        );
    }

    let remote_branch = match payload.r#ref.strip_prefix("refs/heads/") {
        Some(remote_branch) => remote_branch,
        None => {
            invalid_request!(
                StatusCode::BAD_REQUEST,
                "Couldn't get remote branch from payload"
            );
        }
    };

    // The unwrap() is risky, however the check hashmap check above ensures that an invalid repo will
    // never reach this point.
    let local_branch = state
        .repos
        .get(&repo_name)
        .map(|ri| ri.tracked_branch.clone())
        .unwrap();

    if local_branch != remote_branch {
        // Silently return incase request wasn't intentional
        return (StatusCode::BAD_REQUEST, "").into_response();
    };

    let secret = match state.secrets.get(&repo_name) {
        Some(s) => s.trim(),
        None => {
            invalid_request!(
                StatusCode::UNAUTHORIZED,
                format!("No secret for repo {}", repo_name)
            );
        }
    };
    if !verify_signature(secret, signature, &body[..]) {
        invalid_request!(
            StatusCode::UNAUTHORIZED,
            format!("Invalid signature for {}", repo_name)
        );
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

        let _permit = state.active_runners.clone().acquire_owned().await.unwrap();

        info!(
            "Runner started for {}. Active runners: {}/{}",
            &repo_name,
            max_runners - state.active_runners.available_permits() as u8,
            max_runners
        );

        let status = match runner(image, commands, cache_images, continue_on_fail).await {
            Ok((logs, failed, step_failed_on)) => {
                let date = chrono::Local::now().format("%d-%m-%Y %H:%M:%S").to_string();
                let pipeline_entry = PipelineEntry::new(repo_name.clone(), logs, failed, date);

                if state.config.telefy.is_some() {
                    let message = if failed {
                        build_telefy_message!(repo_name, step_failed_on.unwrap_or("".to_string()))
                    } else {
                        build_telefy_message!(repo_name)
                    };
                    telefy::message!(message);
                }

                match database::create_pipeline_entry(pipeline_entry) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Couldn't create database log entry for: {repo_name}. {e}");
                    }
                };
                "successfully"
            }
            Err(e) => {
                error!("Runner for {} failed: {e}", repo_name);
                "unsuccessfully"
            }
        };

        //Fixes inaccurate count of active runners
        drop(_permit);

        info!(
            "Runner for {} finished {}. Active runners: {}/{}",
            repo_name,
            status,
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
