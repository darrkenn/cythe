use std::{convert::Infallible, time::Duration};

use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Sse, sse::Event},
};
use futures_util::{Stream, StreamExt, stream};
use log::error;
use serde_json::json;
use tera::Context;

use crate::{TEMPLATES, app_state::AppState, database::get_latest_entry, routes::RepoQuery};

pub async fn repos(State(state): State<AppState>) -> impl IntoResponse {
    let repos = state.allowed_repos.as_ref();

    let list_html: String = repos
        .iter()
        .map(|repo| format!("<li><a href=\"/repo?name={}\">{}</a></li>", repo, repo))
        .collect::<String>();

    Html(list_html)
}

pub async fn max_runners(State(state): State<AppState>) -> impl IntoResponse {
    let max_runners = state.config.max_active_runners;

    let html = format!("<li>Max runners: {}</li>", max_runners);
    Html(html)
}

pub async fn active_runners(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let max_runners = state.config.max_active_runners;

    let stream = stream::repeat_with(move || state.clone()).then(move |state| async move {
        tokio::time::sleep(Duration::from_secs(1)).await;

        let available_permits = state.active_runners.available_permits();
        let active_runners = max_runners - available_permits as u8;
        let event = Event::default()
            .data(format!("<li>Active runners: {}</li>", active_runners))
            .event("active-runners");

        Ok::<_, Infallible>(event)
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive"),
    )
}

pub async fn latest_entry(repo_query: Query<RepoQuery>) -> impl IntoResponse {
    let latest_entry = match get_latest_entry(repo_query.name.clone()) {
        Ok(pe) => pe,
        Err(e) => {
            error!("Error getting latest_entry: {e}");
            return Html("<h2>Can't get latest logs</h2>".to_string());
        }
    };
    let mut context = Context::new();
    let logs: serde_json::Value = serde_json::from_str(&latest_entry.logs).unwrap_or(json!({}));
    context.insert("id", &latest_entry.id.unwrap_or(0));
    context.insert("name", &latest_entry.name);
    context.insert("logs", &logs);
    context.insert("failed", &latest_entry.failed);
    context.insert("date", &latest_entry.date);

    match TEMPLATES.render("latest_entry.html", &context) {
        Ok(html) => Html(html),
        Err(e) => {
            error!("Template render error: {e}");
            Html("<h1>Template render error</h1>".to_string())
        }
    }
}
