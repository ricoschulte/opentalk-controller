use crate::api::signaling::SignalingHttpModule;
use actix_cors::Cors;
use actix_web::http::header;
use actix_web::web::Data;
use actix_web::{web, App, HttpServer, Scope};
use anyhow::{anyhow, Context, Result};
use api::signaling;
use db::DbInterface;
use fern::colors::{Color, ColoredLevelConfig};
use oidc::OidcContext;
use rustls::internal::pemfile::{certs, rsa_private_keys};
use settings::{Logging, Settings};
use std::fs::File;
use std::io::BufReader;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::ctrl_c;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_amqp::LapinTokioExt;

mod api;
mod cli;
mod db;
mod ha_sync;
mod modules;
mod oidc;
mod settings;

#[macro_use]
extern crate diesel;

#[actix_web::main]
async fn main() -> Result<()> {
    let args = cli::parse_args()?;
    if args.controller_should_start() {
        start_controller(args).await?
    }
    Ok(())
}

async fn start_controller(args: cli::Args) -> Result<()> {
    let settings = settings::load_settings(args)?;

    setup_logging(&settings.logging)?;

    log::info!("Starting K3K Controller");

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
    db::migrations::start_migration(&settings.database)
        .await
        .context("Failed to migrate database")?;

    let (shutdown, _) = broadcast::channel::<()>(1);

    // Connect to RabbitMQ
    let rabbitmq = lapin::Connection::connect(
        &settings.rabbit_mq.url,
        lapin::ConnectionProperties::default().with_tokio(),
    )
    .await
    .context("failed to connect to rabbitmq")?;

    let rabbitmq_channel = rabbitmq
        .create_channel()
        .await
        .context("Could not create rabbitmq channel for ext_http_server")?;

    ha_sync::init(&rabbitmq_channel)
        .await
        .context("Failed to init ha_sync")?;

    // Begin application scope
    {
        // Connect to postgres
        let db_ctx = Arc::new(
            DbInterface::connect(settings.database).context("Failed to connect to database")?,
        );

        // Discover OIDC Provider
        let oidc_ctx = Arc::new(
            OidcContext::from_config(settings.oidc)
                .await
                .context("Failed to initialize OIDC Context")?,
        );

        // Start internal HTTP Server
        let int_http_server = {
            let db_ctx = Arc::downgrade(&db_ctx);
            let oidc_ctx = Arc::downgrade(&oidc_ctx);
            let shutdown = shutdown.clone();

            HttpServer::new(move || {
                // Unwraps cannot panic. Server gets stopped before dropping the Arc.
                let db_ctx = Data::from(db_ctx.upgrade().unwrap());
                let oidc_ctx = Data::from(oidc_ctx.upgrade().unwrap());

                App::new()
                    .app_data(db_ctx)
                    .app_data(oidc_ctx)
                    .app_data(Data::new(shutdown.clone()))
                    .service(api::internal::introspect)
            })
        };

        // Build redis client. Does not check if redis is reachable.
        let redis = redis::Client::open(settings.redis.url).context("Invalid redis url")?;
        let redis_conn = redis::aio::ConnectionManager::new(redis)
            .await
            .context("Failed to create redis connection manager")?;

        // Connect to Janus via rabbitmq
        let mut mcu = {
            let mcu = signaling::McuPool::build(
                settings.room_server,
                rabbitmq_channel.clone(),
                redis_conn.clone(),
            )
            .await
            .context("Failed to connect to Janus WebRTC server")?;

            Arc::new(mcu)
        };

        let mut application = modules::ApplicationBuilder::default();

        {
            let signaling_channel = rabbitmq
                .create_channel()
                .await
                .context("Could not create rabbit mq channel for signaling")?;

            application.add_http_module(
                SignalingHttpModule::new(redis_conn.clone(), signaling_channel)
                    .with_module::<signaling::ce::Echo>(())
                    .with_module::<signaling::ce::Media>(Arc::downgrade(&mcu))
                    .with_module::<signaling::ce::Chat>(())
                    .with_module::<signaling::ee::Chat>(()),
            );
        }

        let application = application.finish();

        // Store the turn server configuration for the turn endpoint
        let turn_servers = Data::new(settings.turn);

        // Start external HTTP Server
        let ext_http_server = {
            let cors = settings.http.cors;

            let rabbitmq_channel = Data::new(rabbitmq_channel);

            let db_ctx = Arc::downgrade(&db_ctx);
            let oidc_ctx = Arc::downgrade(&oidc_ctx);
            let application_weak = Arc::downgrade(&application);
            let shutdown = shutdown.clone();

            HttpServer::new(move || {
                let cors = setup_cors(&cors);

                // Unwraps cannot panic. Server gets stopped before dropping the Arc.
                let db_ctx = Data::from(db_ctx.upgrade().unwrap());
                let oidc_ctx = Data::from(oidc_ctx.upgrade().unwrap());
                let redis_ctx = Data::new(redis_conn.clone());

                let application = application_weak.upgrade().unwrap();

                App::new()
                    .wrap(cors)
                    .app_data(db_ctx.clone())
                    .app_data(oidc_ctx.clone())
                    .app_data(redis_ctx)
                    .app_data(turn_servers.clone())
                    .app_data(Data::new(shutdown.clone()))
                    .app_data(rabbitmq_channel.clone())
                    .service(v1_scope(db_ctx, oidc_ctx))
                    .configure(application.configure())
            })
        };

        let ext_address = (Ipv6Addr::UNSPECIFIED, settings.http.port);
        let int_address = (Ipv6Addr::UNSPECIFIED, settings.http.internal_port);

        let (ext_http_server, int_http_server) = if let Some(tls) = settings.http.tls {
            let config = setup_rustls(tls).context("Failed to setup TLS context")?;

            (
                ext_http_server.bind_rustls(ext_address, config.clone()),
                int_http_server.bind_rustls(int_address, config),
            )
        } else {
            (
                ext_http_server.bind(ext_address),
                int_http_server.bind(int_address),
            )
        };

        let ext_http_server = ext_http_server.with_context(|| {
            format!(
                "Failed to bind external server to {}:{}",
                ext_address.0, ext_address.1
            )
        })?;

        let int_http_server = int_http_server.with_context(|| {
            format!(
                "Failed to bind internal server to {}:{}",
                int_address.0, int_address.1
            )
        })?;

        log::info!("Startup finished");

        let mut ext_server = ext_http_server.disable_signals().run();
        let mut int_server = int_http_server.disable_signals().run();

        let mut reload_signal =
            signal(SignalKind::hangup()).context("Failed to register SIGHUP signal handler")?;

        // Select over both http servers and the SIGTERM event. If any of them return the application
        // will try to gracefully shut down.
        loop {
            tokio::select! {
                _ = &mut ext_server => {
                    log::error!("Http server returned, exiting");
                    break;
                }
                _ = &mut int_server => {
                    log::error!("Internal http server returned, exiting");
                    break;
                }
                _ = ctrl_c() => {
                    log::info!("Got termination signal, exiting");
                    break;
                }
                _ = reload_signal.recv() => {
                    log::info!("Got reload signal, reloading");

                    mcu.try_reconnect().await;
                }
            }
        }
        // ==== Begin shutdown sequence ====

        // Send shutdown signals to all tasks within our application
        let _ = shutdown.send(());

        // then stop HTTP servers
        ext_server.stop(true).await;
        int_server.stop(true).await;

        // Check in a 1 second interval for 10 seconds if all tasks have exited
        // by inspecting the receiver count of the broadcast-channel
        for _ in 0..10 {
            let receiver_count = shutdown.receiver_count();

            if receiver_count > 0 {
                log::debug!("Waiting for {} tasks to be stopped", receiver_count);
                sleep(Duration::from_secs(1)).await;
            }
        }

        // The JanusMcu needs special attention because just dropping it would do no good, since
        // it destroys it session using RabbitMQ channels, which also need to be closed manually.

        // First drop application which contains weak pointers to mcu
        drop(application);

        // Now that all ref-pointers to mcu are dropped use Arc::get_mut to get a mutable
        // reference and call JanusMcu::destroy
        log::debug!("Destroying JanusMcu");
        Arc::get_mut(&mut mcu)
            .expect("Not all ref-pointers to mcu dropped")
            .destroy()
            .await;
    }

    // Close all rabbitmq connections
    // TODO what code and text to use here
    if let Err(e) = rabbitmq.close(0, "shutting down").await {
        log::error!("Failed to close RabbitMQ connections, {}", e);
    }

    drop(rabbitmq);

    if shutdown.receiver_count() > 0 {
        log::error!("Not all tasks stopped. Exiting anyway");
    } else {
        log::info!("All tasks stopped, goodbye!");
    }

    Ok(())
}

fn v1_scope(db_ctx: Data<DbInterface>, oidc_ctx: Data<OidcContext>) -> Scope {
    // the latest version contains the root services
    web::scope("/v1")
        .service(api::v1::auth::login)
        .service(api::v1::auth::oidc_provider)
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
        .allow_any_method()
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
