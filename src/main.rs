use crate::api::signaling;
use crate::settings::{Logging, Settings};
use actix_cors::Cors;
use actix_web::http::{header, Method};
use actix_web::web::Data;
use actix_web::{web, App, HttpServer, Scope};
use anyhow::{anyhow, Context, Result};
use db::DbInterface;
use fern::colors::{Color, ColoredLevelConfig};
use futures_util::future::{select, Either};
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
    let settings = settings::load_settings().context("Failed to load settings from file")?;
    setup_logging(&settings.logging)?;
    log::debug!("Starting K3K Controller with settings {:?}", settings);

    match run_service(settings).await {
        Ok(()) => Ok(()),
        Err(e) => {
            log::error!("Crashed with error: {:?}", e);
            log::logger().flush();
            std::process::exit(-1);
        }
    }
}

async fn run_service(settings: Settings) -> Result<()> {
    if let Err(e) = db::migrations::start_migration(&settings.database).await {
        log::error!(target: "db", "Failed to migrate database: {}", e);
        return Err(e);
    }

    let db_ctx = Data::new(
        DbInterface::connect(settings.database).context("Failed to connect to database")?,
    );

    let oidc_ctx = Data::new(
        OidcContext::from_config(settings.oidc)
            .await
            .context("Failed to initialize OIDC Context")?,
    );

    let mut application = modules::ApplicationBuilder::default();

    signaling::attach(&mut application, settings.room_server, settings.rabbit_mq).await?;

    let application = application.finish();

    let turn_servers = Data::new(settings.turn);

    // Start internal HTTP Server
    let internal_db_ctx = db_ctx.clone();
    let internal_oidc_ctx = oidc_ctx.clone();
    let internal_address = (Ipv6Addr::UNSPECIFIED, settings.http.internal_port);
    let internal_http_server = HttpServer::new(move || {
        App::new()
            .app_data(internal_db_ctx.clone())
            .app_data(internal_oidc_ctx.clone())
            .service(api::internal::introspect)
    });

    // Start external HTTP Server
    let cors = settings.http.cors;
    let ext_address = (Ipv6Addr::UNSPECIFIED, settings.http.port);
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
        let config = setup_rustls(tls).context("Failed to setup TLS context")?;
        (
            ext_http_server.bind_rustls(ext_address, config.clone()),
            internal_http_server.bind_rustls(internal_address, config),
        )
    } else {
        (
            ext_http_server.bind(ext_address),
            internal_http_server.bind(internal_address),
        )
    };

    let ext_http_server = ext_http_server.with_context(|| {
        format!(
            "Failed to bind external server to {}:{}",
            ext_address.0, ext_address.1
        )
    })?;

    let internal_http_server = internal_http_server.with_context(|| {
        format!(
            "Failed to bind internal server to {}:{}",
            internal_address.0, internal_address.1
        )
    })?;

    log::info!("Startup finished");

    match select(ext_http_server.run(), internal_http_server.run()).await {
        Either::Left((external_res, _external_server)) => {
            external_res.context("External server crashed")?
        }
        Either::Right((internal_res, _internal_server)) => {
            internal_res.context("Internal server crashed")?
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
    let colors = ColoredLevelConfig {
        error: Color::Red,
        warn: Color::Yellow,
        info: Color::Green,
        debug: Color::Blue,
        trace: Color::White,
    };

    let logger = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                colors.color(record.level()),
                record.target(),
                message
            ))
        })
        .level(logging.level.to_level_filter());

    match logging.file {
        Some(ref path) => {
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .context("Failed to create or open the logging file")?;

            logger.chain(log_file)
        }
        None => logger,
    }
    .chain(
        fern::Dispatch::new()
            .filter(|metadata| {
                // Reject messages with the `Error` log level.
                metadata.level() != log::LevelFilter::Error
            })
            .chain(std::io::stdout()),
    )
    .chain(
        fern::Dispatch::new()
            .level(log::LevelFilter::Error)
            .chain(std::io::stderr()),
    )
    .apply()
    .context("Failed to setup logging utility")
}

fn setup_rustls(tls: settings::HttpTls) -> Result<rustls::ServerConfig> {
    let mut config = rustls::ServerConfig::new(rustls::NoClientAuth::new());

    let cert_file = File::open(&tls.certificate)
        .with_context(|| format!("Failed to open certificate file {:?}", &tls.certificate))?;
    let certs =
        certs(&mut BufReader::new(cert_file)).map_err(|_| anyhow!("Invalid certificate"))?;

    let private_key_file = File::open(&tls.private_key).with_context(|| {
        format!(
            "Failed to open pkcs8 private key file {:?}",
            &tls.private_key
        )
    })?;
    let mut key = rsa_private_keys(&mut BufReader::new(private_key_file))
        .map_err(|_| anyhow!("Invalid pkcs8 private key"))?;

    config.set_single_cert(certs, key.remove(0))?;

    Ok(config)
}
