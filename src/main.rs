use crate::parse_yml::Config;

mod parse_yml;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let yaml = std::fs::read_to_string("ciythe.yml")?;
    let config: Config = serde_yaml::from_str(&yaml)?;

    let mut docker_file: String = String::from(format!("FROM {}\n WORKDIR /app\n", config.base));

    for step in config.steps {
        if let Some(internal_command) = step.r#use {
            match internal_command.as_str() {
                "ciythe-checkout" => {}
                _ => {}
            }
        }
    }

    Ok(())
}

fn runner(git_url: String) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
