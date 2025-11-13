use axum::{
    extract::State,
    response::{Html, IntoResponse},
};

use crate::app_state::AppState;

pub async fn repos(State(state): State<AppState>) -> impl IntoResponse {
    let repos = state.allowed_repos.as_ref();

    let list_html: String = repos
        .iter()
        .map(|repo| format!("<li><a href=\"/repo?name={}\">{}</a></li>", repo, repo))
        .collect::<String>();

    Html(list_html)
}

pub async fn runners(State(state): State<AppState>) -> impl IntoResponse {
    let active_guard = state.active_runners.lock().await;
    let active_runners: &u8 = &active_guard;
    let max_runners = state.config.max_active_runners;

    let html = format!(
        "<li>Active runners: {}</li><li>Max runners: {}</li>",
        active_runners, max_runners
    );
    Html(html)
}
