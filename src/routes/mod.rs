use axum::{
    Router,
    routing::{get, post},
};
use tower_http::services::ServeFile;

use crate::{
    app_state::AppState,
    routes::{
        api::{repos, runners},
        pages::home,
        webhook::webhook,
    },
};

mod api;
mod pages;
mod stream;
mod webhook;

pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/webhook", post(webhook))
        .route("/api/runners", get(runners))
        .route("/api/repos", get(repos))
        .nest_service("/css", ServeFile::new("webui/static/styles.css"))
        .with_state(app_state)
}
