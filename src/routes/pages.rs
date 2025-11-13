use axum::response::{Html, IntoResponse};

pub async fn home() -> impl IntoResponse {
    let html = tokio::fs::read_to_string("webui/index.html")
        .await
        .unwrap_or_else(|_| "<h1>Couldn't retrieve index.html</h1>".to_string());
    Html(html)
}
