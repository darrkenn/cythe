mod parse_yml;
use crate::parse_yml::{create_docker_file, retrieve_yml};
use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use bollard::{
    Docker, body_full,
    query_parameters::{
        BuildImageOptionsBuilder, CreateContainerOptionsBuilder, RemoveContainerOptions,
        StartContainerOptions, WaitContainerOptions,
    },
    secret::ContainerCreateBody,
};
use chrono::Local;
use dotenv::dotenv;
use futures_util::stream::StreamExt;
use hex::encode;
use hmac::{Hmac, KeyInit, Mac};
use log::{debug, error, info, warn};
use serde::Deserialize;
use sha2::Sha256;
use std::{env, io::Cursor};
use subtle::ConstantTimeEq;
use tar::Builder;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
struct Payload {
    pub r#ref: String,
    pub repository: Repository,
}

#[derive(Deserialize)]
struct Repository {
    pub html_url: String,
    pub full_name: String,
}

async fn webhook(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let secret = env::var("GITHUB_SECRET").unwrap_or_else(|_| "".to_string());
    let signature = match headers.get("X-Hub-Signature-256") {
        Some(sig) => sig.to_str().unwrap_or(""),
        None => {
            warn!("Received a request without a X-Hub-Signature");
            return (StatusCode::UNAUTHORIZED, "").into_response();
        }
    };

    if !verify_signature(&secret, signature, &body[..]) {
        warn!("Received a request with an invalid signature");
        return (StatusCode::UNAUTHORIZED, "").into_response();
    }

    let payload: Payload = match serde_json::from_slice::<Payload>(&body) {
        Ok(p) => p,
        Err(e) => {
            warn!("Received a request with invalid json: {e}");
            return (StatusCode::BAD_REQUEST, "").into_response();
        }
    };

    tokio::task::spawn(async move {
        println!(
            "REF: {}, URL: {}, FULL_NAME: {}",
            payload.r#ref, payload.repository.html_url, payload.repository.full_name
        );

        let tracked_branch = match payload.r#ref.strip_prefix("refs/heads/") {
            Some(tb) => tb,
            None => {
                error!("Malformed JSON or branch isn't included");
                return;
            }
        };

        let cythe_yml = match retrieve_yml(
            payload.repository.full_name,
            tracked_branch.to_string(),
            "https://raw.githubusercontent.com".to_string(),
        )
        .await
        {
            Ok(cy) => cy,
            Err(e) => {
                error!("Error when retrieving cythe.yml: {e}");
                return;
            }
        };
        if cythe_yml.track == tracked_branch {
            let docker_file = match create_docker_file(payload.repository.html_url, cythe_yml) {
                Ok(df) => df,
                Err(e) => {
                    error!("{e}");
                    return;
                }
            };
            println!("{docker_file}");

            let _ = runner(docker_file).await;
        }
    });

    (StatusCode::OK, "").into_response()
}

async fn runner(docker_file: String) -> Result<(), Box<dyn std::error::Error>> {
    let docker = Docker::connect_with_local_defaults()?;
    let name = format!("cythe-{}", Uuid::new_v4());
    let image_name = format!("{}:latest", name);

    let mut a = Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(docker_file.len() as u64);
    header.set_cksum();
    a.append_data(&mut header, "Dockerfile", Cursor::new(docker_file.clone()))?;
    let tar_data = a.into_inner()?;

    let build_options = BuildImageOptionsBuilder::new()
        .dockerfile("Dockerfile")
        .t(&image_name)
        .nocache(true)
        .build();

    let mut build_stream =
        docker.build_image(build_options, None, Some(body_full(tar_data.into())));

    info!("Building image");
    while let Some(result) = build_stream.next().await {
        match result {
            Ok(bi) => debug!("{:?}", bi),

            Err(e) => {
                error!("Error during docker image building: {e}");
                return Err(Box::new(e));
            }
        }
    }
    info!("Successfully built image");

    let config = ContainerCreateBody {
        hostname: Some(name.clone()),
        image: Some(image_name),
        user: Some("root".to_string()),
        ..Default::default()
    };

    let options = CreateContainerOptionsBuilder::new().name(&name).build();
    let container = docker.create_container(Some(options), config).await?;
    info!("Created container: {}", container.id);

    docker
        .start_container(&name, None::<StartContainerOptions>)
        .await?;
    info!("Started container: {}", container.id);

    let mut wait_stream = docker.wait_container(&name, None::<WaitContainerOptions>);

    while let Some(result) = wait_stream.next().await {
        match result {
            Ok(status) => {
                info!("Container exited with status: {:?}", status);
            }
            Err(e) => {
                error!("{e}");
                return Err(Box::new(e));
            }
        }
    }

    docker
        .remove_container(&name, None::<RemoveContainerOptions>)
        .await?;
    info!("Removed container: {}", container.id);

    Ok(())
}

fn verify_signature(secret: &str, signature: &str, body_bytes: &[u8]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "9e989623257bb798879f668168fa1f3efbfce4c458a985896ef1d414fd6e733c";
    const BODY: &str = "test";

    #[tokio::test]
    async fn test_succesful_signature_verification() {
        let signature = "sha256=6b1f54cb6c6305de8a244a2e8e201ed9df89a194d83c4d290c45932c9229b3a8";
        assert!(verify_signature(SECRET, signature, BODY.as_bytes()));
    }

    #[tokio::test]
    async fn test_unsuccesful_signature_verifcation() {
        let signature = "sha256=4a2e8e201ed9df89a194d83c4d290c45932c9229b3a814244214214124214241";
        assert!(!verify_signature(SECRET, signature, BODY.as_bytes()))
    }
}
