use bollard::{
    Docker,
    exec::StartExecResults,
    query_parameters::{
        CreateContainerOptionsBuilder, CreateImageOptionsBuilder, InspectContainerOptionsBuilder,
        RemoveContainerOptions, RemoveImageOptions, StartContainerOptions, StopContainerOptions,
    },
    secret::{ContainerCreateBody, ContainerCreateResponse, ExecConfig},
};
use futures_util::StreamExt;
use log::{error, info};

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

pub async fn pull_image(docker: &Docker, image: &str) -> Result<(), Box<dyn std::error::Error>> {
    if docker.inspect_image(image).await.is_ok() {
        println!("Image exists");
        return Ok(());
    };

    let options = CreateImageOptionsBuilder::new().from_image(image).build();

    let mut pull_stream = docker.create_image(Some(options), None, None);

    info!("Pulling image: {}", image);
    while let Some(result) = pull_stream.next().await {
        match result {
            Ok(_) => {}
            Err(e) => {
                error!("Error during image pull: {e}");
                return Err(Box::new(e));
            }
        }
    }

    info!("Successfully pulled image: {image}");
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
    cache_images: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    docker
        .remove_container(name, None::<RemoveContainerOptions>)
        .await?;
    info!("Removed container: {}", container.id);

    if !cache_images {
        docker
            .remove_image(image_name, None::<RemoveImageOptions>, None)
            .await?;
        info!("Removed image: {}", &image_name);
    }
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
