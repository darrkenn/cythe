use std::io::Cursor;

use bollard::{
    Docker, body_full,
    exec::StartExecResults,
    query_parameters::{
        BuildImageOptionsBuilder, CreateContainerOptionsBuilder, InspectContainerOptionsBuilder,
        RemoveContainerOptions, RemoveImageOptions, StartContainerOptions, StopContainerOptions,
    },
    secret::{ContainerCreateBody, ContainerCreateResponse, ExecConfig},
};
use futures_util::StreamExt;
use log::{error, info};
use tar::Builder;

#[derive(Debug)]
pub enum ContainerError {
    NotRunning(String),
    DockerErr(bollard::errors::Error),
}
impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::NotRunning(s) => write!(f, "Container {} not running", s),
            ContainerError::DockerErr(err) => write!(f, "Docker error: {}", err),
        }
    }
}
impl std::error::Error for ContainerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ContainerError::DockerErr(err) => Some(err),
            _ => None,
        }
    }
}

impl From<bollard::errors::Error> for ContainerError {
    fn from(value: bollard::errors::Error) -> Self {
        ContainerError::DockerErr(value)
    }
}

pub async fn build_image(
    docker: &Docker,
    image_name: &str,
    docker_file: String,
) -> Result<(), Box<dyn std::error::Error>> {
    if docker.inspect_image(image_name).await.is_ok() {
        println!("Image exists, stopping the build");
        return Ok(());
    };
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
        tty: Some(true),
        ..Default::default()
    };

    let options = CreateContainerOptionsBuilder::new().name(name).build();
    let container = docker.create_container(Some(options), config).await?;
    info!("Created container: {}", container.id);

    docker
        .start_container(name, None::<StartContainerOptions>)
        .await?;
    info!("Started container: {}", container.id);

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

pub async fn stop_container(docker: &Docker, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    docker
        .stop_container(name, None::<StopContainerOptions>)
        .await?;
    info!("Stopped container {}", name);
    Ok(())
}

pub async fn inspect_container(docker: &Docker, name: &str) -> Result<(), ContainerError> {
    let options = InspectContainerOptionsBuilder::new().build();
    let container_info = docker.inspect_container(name, Some(options)).await?;

    if container_info.state.as_ref().and_then(|s| s.running) != Some(true) {
        return Err(ContainerError::NotRunning(name.to_string()));
    }
    Ok(())
}

pub async fn run_command(
    docker: &Docker,
    name: &str,
    command: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    inspect_container(docker, name).await?;
    let config = ExecConfig {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        cmd: Some(command),
        ..Default::default()
    };
    let exec = docker.create_exec(name, config).await?.id;
    if let StartExecResults::Attached { mut output, .. } = docker.start_exec(&exec, None).await? {
        while let Some(Ok(msg)) = output.next().await {
            print!("{msg}");
        }
    } else {
        unreachable!()
    }
    Ok(())
}
