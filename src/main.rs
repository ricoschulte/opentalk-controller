use actix_cors::Cors;
use actix_web::http::{header, Method};
use actix_web::web::Data;
use actix_web::{web, App, HttpServer, Scope};
use anyhow::{Context, Result};
use db::DbInterface;
use modules::http::ws::{Echo, WebSocketHttpModule};
use oidc::OidcContext;
use std::net::Ipv4Addr;

mod api;
mod db;
mod modules;
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

    let db_ctx = Data::new(
        DbInterface::connect(settings.database)
            .context("Failed to initialize Database connection")?,
    );

    // Discover OIDC Provider
    let oidc_ctx = Data::new(
        OidcContext::from_config(settings.oidc)
            .await
            .context("Failed to initialize OIDC Context")?,
    );

    let mut application = modules::ApplicationBuilder::default();
    let mut signaling = WebSocketHttpModule::new("/signaling", &["k3k-signaling-json-v1"]);
    signaling.add_module::<Echo>(());
    application.add_http_module(signaling);
    let application = application.finish();

    // Start HTTP Server
    let cors = settings.http.cors;
    let http_server = HttpServer::new(move || {
        let cors = setup_cors(&cors);

        App::new()
            .wrap(cors)
            .app_data(db_ctx.clone())
            .app_data(oidc_ctx.clone())
            .service(v1_scope(db_ctx.clone(), oidc_ctx.clone()))
            .configure(application.configure())
    });

    let http_server = http_server.bind((Ipv4Addr::UNSPECIFIED, settings.http.port))?;

    http_server.run().await?;

    Ok(())
}

fn v1_scope(db_ctx: Data<DbInterface>, oidc_ctx: Data<OidcContext>) -> Scope {
    // the latest version contains the root services
    web::scope("/v1")
        .service(api::v1::auth::login)
        .service(api::v1::auth::oidc_providers)
        .service(
            // empty scope to differentiate between auth endpoints
            web::scope("")
                .wrap(api::v1::middleware::oidc_auth::OidcAuth { db_ctx, oidc_ctx })
                .service(api::v1::users::all)
                .service(api::v1::users::set_current_user_profile)
                .service(api::v1::users::current_user_profile)
                .service(api::v1::users::user_details)
                .service(api::v1::rooms::owned)
                .service(api::v1::rooms::new)
                .service(api::v1::rooms::modify)
                .service(api::v1::rooms::get)
                .service(api::v1::rooms::start),
        )
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
