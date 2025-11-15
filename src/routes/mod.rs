use axum::{
    Router,
    routing::{get, post},
};
use tower_http::services::{ServeDir, ServeFile};

use crate::{
    app_state::AppState,
    routes::{
        api::{active_runners, max_runners, repos},
        pages::{home, repo},
        webhook::webhook,
    },
};

mod api;
mod pages;
mod webhook;

pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/repo", get(repo))
        .route("/webhook", post(webhook))
        .route("/api/max-runners", get(max_runners))
        .route("/api/active-runners", get(active_runners))
        .route("/api/repos", get(repos))
        .nest_service("/favicon.ico", ServeFile::new("webui/favicon.ico"))
        .nest_service("/static/", ServeDir::new("webui/static/"))
        .nest_service("/js/", ServeDir::new("webui/js"))
        .with_state(app_state)
}
