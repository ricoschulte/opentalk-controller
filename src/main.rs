use crate::oidc::OidcContext;
use actix_cors::Cors;
use actix_web::http::{header, Method};
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use anyhow::{Context, Result};
use std::net::Ipv4Addr;

mod api;
mod db;
mod oidc;
mod settings;

#[macro_use]
extern crate diesel;

#[actix_web::main]
async fn main() -> Result<()> {
    setup_logging()?;

    let settings = settings::Settings::load("config.toml")?;

    // Run database migration
    db::migrations::start_migration(&settings.database).await?;

    // Discover OIDC Provider
    let oidc_ctx = Data::new(
        OidcContext::from_config(settings.oidc)
            .await
            .context("Failed to initialize OIDC Context")?,
    );

    // Start HTTP Server
    let cors = settings.http.cors;
    let http_server = HttpServer::new(move || {
        let cors = setup_cors(&cors);

        App::new()
            .wrap(cors)
            .app_data(oidc_ctx.clone())
            .service(api::api_example)
            .service(api::auth::login)
    });

    let http_server = http_server.bind((Ipv4Addr::UNSPECIFIED, settings.http.port))?;

    http_server.run().await?;

    Ok(())
}

fn setup_cors(settings: &settings::Cors) -> Cors {
    let mut cors = Cors::default();

    for origin in &settings.allowed_origin {
        cors = cors.allowed_origin(&origin)
    }

    cors.allowed_header(header::CONTENT_TYPE)
        .allowed_header(header::AUTHORIZATION)
        .allowed_methods(&[Method::POST])
}

fn setup_logging() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply()
        .context("Failed to setup logging utility")
}
