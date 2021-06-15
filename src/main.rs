use crate::settings::Logging;
use actix_cors::Cors;
use actix_web::http::{header, Method};
use actix_web::web::Data;
use actix_web::{web, App, HttpServer, Scope};
use anyhow::{anyhow, Context, Result};
use db::DbInterface;
use futures_util::future::{select, Either};
use modules::http::ws::{Echo, WebSocketHttpModule};
use oidc::OidcContext;
use rustls::internal::pemfile::{certs, rsa_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::net::Ipv6Addr;

mod api;
mod db;
mod modules;
mod oidc;
mod settings;

#[macro_use]
extern crate diesel;

#[actix_web::main]
async fn main() -> Result<()> {
    let settings = settings::load_settings()?;
    setup_logging(&settings.logging)?;
    log::debug!("Starting K3K Controller with settings {:?}", settings);
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

    let turn_servers = Data::new(settings.turn);

    // Start internal HTTP Server
    let internal_db_ctx = db_ctx.clone();
    let internal_oidc_ctx = oidc_ctx.clone();
    let internal_http_server = HttpServer::new(move || {
        App::new()
            .app_data(internal_db_ctx.clone())
            .app_data(internal_oidc_ctx.clone())
            .service(api::internal::introspect)
    });

    // Start external HTTP Server
    let cors = settings.http.cors;
    let ext_http_server = HttpServer::new(move || {
        let cors = setup_cors(&cors);

        App::new()
            .wrap(cors)
            .app_data(db_ctx.clone())
            .app_data(oidc_ctx.clone())
            .app_data(turn_servers.clone())
            .service(v1_scope(db_ctx.clone(), oidc_ctx.clone()))
            .configure(application.configure())
    });

    let (ext_http_server, internal_http_server) = if let Some(tls) = settings.http.tls {
        let config = setup_rustls(tls)?;

        (
            ext_http_server
                .bind_rustls((Ipv6Addr::UNSPECIFIED, settings.http.port), config.clone())?,
            internal_http_server
                .bind_rustls((Ipv6Addr::UNSPECIFIED, settings.http.internal_port), config)?,
        )
    } else {
        (
            ext_http_server.bind((Ipv6Addr::UNSPECIFIED, settings.http.port))?,
            internal_http_server.bind((Ipv6Addr::UNSPECIFIED, settings.http.internal_port))?,
        )
    };

    match select(ext_http_server.run(), internal_http_server.run()).await {
        Either::Left((external_res, _external_server)) => {
            external_res.expect("External server error: {}")
        }
        Either::Right((internal_res, _internal_server)) => {
            internal_res.expect("Internal server error: {}")
        }
    };

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
                .service(api::v1::rooms::start)
                .service(api::v1::turn::get),
        )
}

fn setup_cors(settings: &settings::HttpCors) -> Cors {
    let mut cors = Cors::default();

    for origin in &settings.allowed_origin {
        cors = cors.allowed_origin(&origin)
    }

    cors.allowed_header(header::CONTENT_TYPE)
        .allowed_header(header::AUTHORIZATION)
        .allowed_methods(&[Method::POST])
}

fn setup_logging(logging: &Logging) -> Result<()> {
    let logger = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(logging.level.to_level_filter());

    match logging.output {
        Some(ref path) => {
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;

            logger.chain(log_file)
        }
        None => logger.chain(std::io::stdout()),
    }
    .apply()
    .context("Failed to setup logging utility")
}

fn setup_rustls(tls: settings::HttpTls) -> Result<rustls::ServerConfig> {
    let mut config = rustls::ServerConfig::new(rustls::NoClientAuth::new());

    let certs = certs(&mut BufReader::new(File::open(tls.certificate)?))
        .map_err(|_| anyhow!("Failed to read certificate file"))?;

    let mut key = rsa_private_keys(&mut BufReader::new(File::open(tls.private_key)?))
        .map_err(|_| anyhow!("Failed to read pkcs8 private key file"))?;

    config.set_single_cert(certs, key.remove(0))?;

    Ok(config)
}
