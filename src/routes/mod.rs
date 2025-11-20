use axum::{
    Router,
    routing::{get, post},
};
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    app_state::AppState,
    routes::{
        api::{active_runners, latest_entry, max_runners, repos},
        pages::{home, repo},
        webhook::webhook,
    },
};

mod api;
mod pages;
mod webhook;

#[derive(Deserialize)]
pub struct RepoQuery {
    pub name: String,
}

pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/repo", get(repo))
        .route("/webhook", post(webhook))
        .route("/api/max-runners", get(max_runners))
        .route("/api/active-runners", get(active_runners))
        .route("/api/repos", get(repos))
        .route("/api/latest_entry", get(latest_entry))
        .nest_service("/favicon.ico", ServeFile::new("webui/favicon.ico"))
        .nest_service("/static/", ServeDir::new("webui/static/"))
        .nest_service("/js/", ServeDir::new("webui/js"))
        .with_state(app_state)
}

#[cfg(debug_assertions)]
pub fn create_router_debug(app_state: AppState) -> Router {
    use crate::routes::webhook::webhook_debug;

    Router::new()
        .route("/", get(home))
        .route("/repo", get(repo))
        .route("/webhook_debug", post(webhook_debug))
        .route("/api/max-runners", get(max_runners))
        .route("/api/active-runners", get(active_runners))
        .route("/api/repos", get(repos))
        .route("/api/latest_entry", get(latest_entry))
        .nest_service("/favicon.ico", ServeFile::new("webui/favicon.ico"))
        .nest_service("/static/", ServeDir::new("webui/static/"))
        .nest_service("/js/", ServeDir::new("webui/js"))
        .with_state(app_state)
}
