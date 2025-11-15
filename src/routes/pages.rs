use axum::{
    extract::Query,
    response::{Html, IntoResponse},
};
use log::error;
use serde::Deserialize;
use tera::Context;

use crate::TEMPLATES;

#[derive(Deserialize)]
pub struct RepoQuery {
    pub name: String,
}

pub async fn home() -> impl IntoResponse {
    let html = tokio::fs::read_to_string("webui/index.html")
        .await
        .unwrap_or_else(|_| "<h1>Couldn't retrieve index.html</h1>".to_string());
    Html(html)
}
pub async fn repo(repo_query: Query<RepoQuery>) -> impl IntoResponse {
    let mut context = Context::new();
    let strings: Vec<&str> = repo_query.name.split("/").collect();
    let org = strings[0];
    let name = strings[1];
    context.insert("org", org);
    context.insert("repo", name);

    match TEMPLATES.render("repo.html", &context) {
        Ok(html) => Html(html),
        Err(e) => {
            error!("Template render error: {e}");
            Html("<h1>Template render error</h1>".to_string())
        }
    }
}
