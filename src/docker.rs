use std::io::Cursor;

use bollard::{
    Docker, body_full,
    query_parameters::{
        BuildImageOptionsBuilder, CreateContainerOptionsBuilder, RemoveContainerOptions,
        RemoveImageOptions, StartContainerOptions, WaitContainerOptions,
    },
    secret::{ContainerCreateBody, ContainerCreateResponse},
};
use futures_util::StreamExt;
use log::{error, info};
use tar::Builder;

pub async fn build_image(
    docker: &Docker,
    image_name: &str,
    docker_file: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut a = Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(docker_file.len() as u64);
    header.set_cksum();
    a.append_data(&mut header, "Dockerfile", Cursor::new(docker_file.clone()))?;
    let tar_data = a.into_inner()?;

    let build_options = BuildImageOptionsBuilder::new()
        .dockerfile("Dockerfile")
        .t(image_name)
        .nocache(true)
        .build();

    let mut build_stream =
        docker.build_image(build_options, None, Some(body_full(tar_data.into())));

    info!("Building image");
    while let Some(result) = build_stream.next().await {
        match result {
            Ok(_) => {}

            Err(e) => {
                error!("Error during docker image building: {e}");
                return Err(Box::new(e));
            }
        }
    }
    info!("Successfully built image");
    Ok(())
}

pub async fn start_container(
    docker: &Docker,
    name: &str,
    image_name: &str,
) -> Result<ContainerCreateResponse, Box<dyn std::error::Error>> {
    let config = ContainerCreateBody {
        hostname: Some(name.to_string()),
        image: Some(image_name.to_string()),
        user: Some("root".to_string()),
        ..Default::default()
    };

    let options = CreateContainerOptionsBuilder::new().name(name).build();
    let container = docker.create_container(Some(options), config).await?;
    info!("Created container: {}", container.id);

    docker
        .start_container(name, None::<StartContainerOptions>)
        .await?;
    info!("Started container: {}", container.id);

    let mut wait_stream = docker.wait_container(name, None::<WaitContainerOptions>);

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
    Ok(container)
}

pub async fn cleanup_docker(
    docker: &Docker,
    container: ContainerCreateResponse,
    name: &str,
    image_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    docker
        .remove_container(name, None::<RemoveContainerOptions>)
        .await?;
    info!("Removed container: {}", container.id);

    docker
        .remove_image(image_name, None::<RemoveImageOptions>, None)
        .await?;
    info!("Removed image: {}", &image_name);
    Ok(())
}
