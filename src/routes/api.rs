use std::{convert::Infallible, time::Duration};

use axum::{
    extract::State,
    response::{Html, IntoResponse, Sse, sse::Event},
};
use futures_util::{Stream, StreamExt, stream};

use crate::app_state::AppState;

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
