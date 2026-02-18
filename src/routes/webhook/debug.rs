use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use log::{error, info, warn};
use serde::Deserialize;

use crate::{
    app_state::AppState,
    build_telefy_message,
    database::{self, PipelineEntry},
    invalid_request,
    runner::runner,
    yaml::{CytheYAML, parse_yaml, retrieve_yaml},
};

#[derive(Deserialize)]
pub struct DebugWebhookQuery {
    name: String,
    tracked_branch: String,
    git_url: String,
    cythe_yml_location: Option<String>,
}

pub async fn webhook(
    State(state): State<AppState>,
    repo_query: Query<DebugWebhookQuery>,
) -> impl IntoResponse {
    let repo_name = repo_query.name.clone();
    let remote_branch = repo_query.tracked_branch.clone();
    let git_url = repo_query.git_url.clone();
    let cythe_yml_location = repo_query.cythe_yml_location.clone();
    let repo_secrets = state.repos.get(&repo_name).unwrap().secrets.clone();

    let local_branch = match state.repos.get(&repo_name) {
        Some(ri) => ri.tracked_branch.clone(),
        None => {
            warn!(
                "Available repos: {:?}",
                state.repos.keys().collect::<Vec<_>>()
            );
            invalid_request!(
                StatusCode::UNAUTHORIZED,
                format!("Repository {} not found", repo_name)
            );
        }
    };
    if local_branch != remote_branch {
        invalid_request!(
            StatusCode::UNAUTHORIZED,
            format!(
                "Remote branch {} does not match local branch {} on {}",
                remote_branch, local_branch, repo_name
            )
        );
    };

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
                    let prepended_message = format!(
                        r#"
DEBUG MODE
{}
                    "#,
                        message
                    );
                    telefy::message!(prepended_message);
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
