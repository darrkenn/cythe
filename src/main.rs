mod parse_yml;
use crate::parse_yml::Config;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use chrono::Local;
use dotenv::dotenv;
use hex::encode;
use hmac::{Hmac, KeyInit, Mac};
use log::{error, info, warn};
use serde::Deserialize;
use sha2::Sha256;
use std::env;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
struct Payload {
    pub repository: Repository,
}

#[derive(Deserialize)]
struct Repository {
    pub html_url: String,
}

async fn webhook(headers: HeaderMap, body: Body) -> impl IntoResponse {
    let secret = env::var("GITHUB_SECRET").unwrap_or_else(|_| "".to_string());

    let signature = match headers.get("X-Hub-Signature-256") {
        Some(sig) => sig.to_str().unwrap_or(""),
        None => {
            warn!("Received a request without a X-Hub-Signature");
            return (StatusCode::UNAUTHORIZED, "").into_response();
        }
    };

    let body_bytes = match to_bytes(body, usize::MAX).await {
        Ok(bb) => bb,
        Err(e) => {
            error!("Couldnt convert body to bytes: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "").into_response();
        }
    };

    if !verifiy_signature(&secret, signature, &body_bytes[..]) {
        warn!("Received a request with an invalid signature");
        return (StatusCode::UNAUTHORIZED, "").into_response();
    }

    let payload: Payload = match serde_json::from_slice::<Payload>(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            warn!("Received a request with invalid json");
            return (StatusCode::BAD_REQUEST, "").into_response();
        }
    };

    println!("REPOSITORY URL: {}", payload.repository.html_url);

    (StatusCode::OK, "PUSH EVENT RECEIVED").into_response()
}

fn verifiy_signature(secret: &str, signature: &str, body_bytes: &[u8]) -> bool {
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

fn setup_logger() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .chain(fern::log_file("cythe.log")?)
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}]: {}",
                Local::now().format("%d-%m-%Y %H:%M:%S"),
                record.target(),
                record.level(),
                message
            ))
        })
        .apply()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger().expect("Couldnt setup logger");
    info!("Staring up cythe");
    dotenv().ok();

    let app = Router::new().route("/webhook", post(webhook));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:6143").await.unwrap();

    info!("cythe up and running at 0.0.0.0:6143");
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
