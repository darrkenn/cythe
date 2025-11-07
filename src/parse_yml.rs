use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Step {
    pub name: String,
    pub run: Option<String>,
    pub r#use: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub base: String,
    pub steps: Vec<Step>,
}
