use bollard::Docker;
use log::{error, info};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    docker::{cleanup_docker, pull_image, run_command, start_container, stop_container},
    yaml::RunStepCommand,
};

#[derive(Serialize)]
struct StepMessage {
    pub name: String,
    pub messages: serde_json::Value,
}

pub async fn runner(
    image: String,
    commands: Vec<RunStepCommand>,
    cache_images: bool,
    continue_on_fail: bool,
) -> Result<(String, bool, Option<String>), anyhow::Error> {
    let docker = Docker::connect_with_unix_defaults()?;
    let name = format!("cythe-{}", Uuid::new_v4());
    let mut failed = false;
    let mut step_failed_on: Option<String> = None;

    match pull_image(&docker, &image).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(anyhow::Error::from(e));
        }
    }

    match start_container(&docker, &name, &image).await {
        Ok(c) => c,
        Err(e) => {
            error!("{e}");
            return Err(anyhow::Error::from(e));
        }
    };

    let mut step_messages: Vec<StepMessage> = Vec::new();
    for step in commands {
        let (step_name, command) = (step.name, step.command);
        let command_vec: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
        info!("Running step {}", &step_name);
        let (messages, success) = match run_command(&docker, &name, command_vec).await {
            Ok((m, s)) => (m, s),
            Err(e) => return Err(anyhow::Error::from(e)),
        };
        let step_message = StepMessage {
            name: step_name.clone(),
            messages: json!(messages),
        };

        step_messages.push(step_message);

        if !success {
            failed = true;
            step_failed_on = Some(step_name);
            if !continue_on_fail {
                break;
            }
        }
    }

    let logs = json!({"steps": step_messages}).to_string();
    drop(step_messages);

    match stop_container(&docker, &name).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(anyhow::Error::from(e));
        }
    }

    match cleanup_docker(&docker, &name, &image, cache_images).await {
        Ok(_) => {}
        Err(e) => {
            error!("{e}");
            return Err(anyhow::Error::from(e));
        }
    };

    Ok((logs, failed, step_failed_on))
}
