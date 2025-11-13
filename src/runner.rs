use bollard::Docker;
use log::error;
use uuid::Uuid;

use crate::{
    docker::{cleanup_docker, pull_image, run_command, start_container, stop_container},
    parse_yml::RunStepCommand,
};

pub async fn runner(
    image: String,
    commands: Vec<RunStepCommand>,
    cache_images: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let docker = Docker::connect_with_local_defaults()?;
    let name = format!("cythe-{}", Uuid::new_v4());

    match pull_image(&docker, &image).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    }

    let container = match start_container(&docker, &name, &image).await {
        Ok(c) => c,
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    };

    for step in commands {
        let (_step_name, command) = (step.name, step.command);
        let command_vec: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
        println!("Container name: {}", name);
        run_command(&docker, &name, command_vec).await?;
    }

    match stop_container(&docker, &name).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    }

    match cleanup_docker(&docker, container, &name, &image, cache_images).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(e);
        }
    };

    Ok(())
}
