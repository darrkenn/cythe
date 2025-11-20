mod docker;
mod parse_yml;
mod routes;
mod runner;
use chrono::Local;
use lazy_static::lazy_static;
use log::{LevelFilter, info};
use std::str::FromStr;
use tera::Tera;
mod app_state;
mod database;

use crate::{
    app_state::load_app_state,
    database::create_tables,
    routes::{create_router, create_router_debug},
};

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        let mut tera = match Tera::new("webui/templates/**/*") {
            Ok(t) => t,
            Err(e) => {
                panic!("Error parsing templates: {}", e);
            }
        };
        tera.autoescape_on(vec![".html"]);
        tera
    };
}

fn setup_logger(log_level: &str) -> Result<(), fern::InitError> {
    let level = LevelFilter::from_str(log_level).unwrap_or(LevelFilter::Info);
    fern::Dispatch::new()
        .level(level)
        .filter(|metadata| metadata.level() != log::Level::Debug)
        .chain(std::io::stdout())
        .chain(fern::log_file("/var/log/cythe/cythe.log")?)
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
    let app_state = load_app_state()?;
    let repos = app_state.repos.as_ref().to_owned();
    create_tables(repos.keys().cloned().collect())?;
    setup_logger(&app_state.config.log_level).expect("Couldnt setup logger");
    info!("Starting up cythe");

    let router = if cfg!(debug_assertions) {
        println!("Running in debug mode");
        create_router_debug(app_state)
    } else {
        println!("Running in debug mode");
        create_router(app_state)
    };

    let listener = tokio::net::TcpListener::bind("0.0.0.0:6143").await.unwrap();
    info!("cythe available at 0.0.0.0:6143");
    axum::serve(listener, router).await.unwrap();
    Ok(())
}
