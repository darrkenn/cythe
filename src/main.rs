mod docker;
mod parse_yml;
mod routes;
mod runner;
use chrono::Local;
use lazy_static::lazy_static;
use log::{LevelFilter, info};
use std::{env, str::FromStr};
use tera::Tera;
mod app_state;
mod database;

use crate::routes::create_router_debug;
use crate::{app_state::load_app_state, database::create_tables, routes::create_router};

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
    let args: Vec<String> = env::args().skip(1).collect();

    let app_state = load_app_state()?;
    let repos = app_state.repos.as_ref().to_owned();
    create_tables(repos.keys().cloned().collect())?;
    setup_logger(&app_state.config.log_level).expect("Couldnt setup logger");

    let router = match args.first().map(|s| s.as_str()) {
        None => {
            info!("Running in release mode");
            create_router(app_state)
        }
        Some("--debug") => {
            info!("Running in debug mode");
            create_router_debug(app_state)
        }
        Some(opt) => panic!("Not a supported run option: {}", opt),
    };

    let listener = tokio::net::TcpListener::bind("0.0.0.0:6143").await.unwrap();
    info!("cythe available at 0.0.0.0:6143");
    axum::serve(listener, router).await.unwrap();
    Ok(())
}
