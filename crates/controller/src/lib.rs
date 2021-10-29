//! Extensible core library of the *K3K Controller*
//!
//! # Example
//!
//! ```no_run
//! use k3k_controller_core::Controller;
//! use anyhow::Result;
//!
//! #[actix_web::main]
//! async fn main()  {
//!     k3k_controller_core::try_or_exit(run()).await;
//! }
//!
//! async fn run() -> Result<()> {
//!    if let Some(controller) = Controller::create().await? {
//!         controller.run().await?;
//!     }
//!
//!     Ok(())
//! }
//! ```

use crate::settings::{Settings, SharedSettings};
use crate::trace::ReducedSpanBuilder;
use actix_cors::Cors;
use actix_web::http::header;
use actix_web::web::Data;
use actix_web::{web, App, HttpServer, Scope};
use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwap;
use breakout::BreakoutRooms;
use db::DbInterface;
use oidc::OidcContext;
use prelude::*;
use redis::aio::ConnectionManager;
use rustls::internal::pemfile::{certs, rsa_private_keys};
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
use tracing_actix_web::TracingLogger;

#[cfg(not(doc))]
mod api;
#[cfg(doc)]
pub mod api;

mod cli;
mod ha_sync;
mod oidc;
mod trace;

pub mod db;
pub mod settings;

#[macro_use]
extern crate diesel;

pub mod prelude {
    pub use crate::api::signaling::prelude::*;
    pub use crate::api::Participant;
    pub use crate::{impl_from_redis_value_de, impl_to_redis_args, impl_to_redis_args_se};

    // re-export commonly used crates to reduce dependency management in module-crates
    pub use actix_web;
    pub use anyhow;
    pub use async_trait;
    pub use chrono;
    pub use displaydoc;
    pub use futures;
    pub use lapin;
    pub use log;
    pub use r3dlock;
    pub use redis;
    pub use serde_json;
    pub use thiserror;
    pub use tokio;
    pub use tokio_stream;
    pub use tracing;
    pub use url;
    pub use uuid;
}

/// Custom version of `actix_web::web::block` which retains the current tracing span
pub async fn block<F, R>(f: F) -> Result<R, actix_web::error::BlockingError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();

    let fut = actix_rt::task::spawn_blocking(move || span.in_scope(f));

    fut.await.map_err(|_| actix_web::error::BlockingError)
}

/// Wrapper of the main function. Correctly outputs the error to the logging utility or stderr.
pub async fn try_or_exit<T, F>(f: F) -> T
where
    F: std::future::Future<Output = Result<T>>,
{
    match f.await {
        Ok(ok) => {
            trace::destroy().await;

            ok
        }
        Err(err) => {
            if log::log_enabled!(log::Level::Error) {
                log::error!("Crashed with error: {:?}", err);
            } else {
                eprintln!("Crashed with error: {:?}", err);
            }

            trace::destroy().await;

            std::process::exit(-1);
        }
    }
}

/// Controller struct representation containing all fields required to extend and drive the controller
pub struct Controller {
    /// Settings loaded on [Controller::create]
    pub startup_settings: Arc<Settings>,

    /// Cloneable shared settings, can be used to reload settings from, when receiving the `reload` signal.
    pub shared_settings: SharedSettings,

    // CLI arguments
    args: cli::Args,

    db: Arc<DbInterface>,
    oidc: Arc<OidcContext>,

    /// RabbitMQ connection, can be used to create channels
    pub rabbitmq: Arc<lapin::Connection>,

    /// RabbitMQ channel, can be cloned and used directly.
    ///
    /// If RabbitMQ is used extensively by a module it should create its own channel.
    pub rabbitmq_channel: lapin::Channel,

    /// Cloneable redis connection manager, can be used to write/read to the controller's redis.
    pub redis: ConnectionManager,

    /// Reload signal which can be triggered by a user.
    /// When received a module should try to re-read it's config and act accordingly.
    ///
    /// `controller.reload.subscribe()` to receive a receiver to the reload-signal.
    pub reload: broadcast::Sender<()>,

    /// Shutdown signal which is triggered when the controller is exiting, either because a fatal error occurred
    /// or a user requested the shutdown.
    ///
    /// `controller.shutdown.subscribe()` to receive a receiver to the reload-signal.
    /// The controller will wait up 10 seconds before forcefully shutting down.
    /// It is tracking the shutdown progress by counting the shutdown-receiver count.
    pub shutdown: broadcast::Sender<()>,

    /// List of signaling modules registered to the controller.
    ///
    /// Can and should be used to extend the controllers signaling endpoint's capabilities.
    pub signaling: SignalingModules,
}

impl Controller {
    /// Tries to create a controller from CLI arguments and then the settings.
    ///
    /// This can return Ok(None) which would indicate that the controller executed a CLI
    /// subprogram (e.g. `--reload`) and must now exit.
    ///
    /// Otherwise it will return itself which can be modified and then run using [`Controller::run`]

    pub async fn create() -> Result<Option<Self>> {
        let args = cli::parse_args()?;

        // Some args run commands by them self and thus should exit here
        if !args.controller_should_start() {
            return Ok(None);
        }

        let settings = settings::load_settings(&args)?;

        trace::init(&settings.logging)?;

        log::info!("Starting K3K Controller");

        let controller = Self::init(settings, args).await?;

        Ok(Some(controller))
    }

    #[tracing::instrument(err, skip(settings, args))]
    async fn init(settings: Settings, args: cli::Args) -> Result<Self> {
        let settings = Arc::new(settings);
        let shared_settings: SharedSettings = Arc::new(ArcSwap::from(settings.clone()));

        db::migrations::migrate_from_url(&settings.database.url)
            .await
            .context("Failed to migrate database")?;

        let rabbitmq = Arc::new(
            lapin::Connection::connect(
                &settings.rabbit_mq.url,
                lapin::ConnectionProperties::default().with_tokio(),
            )
            .await
            .context("failed to connect to rabbitmq")?,
        );

        let rabbitmq_channel = rabbitmq
            .create_channel()
            .await
            .context("Could not create rabbitmq channel")?;

        ha_sync::init(&rabbitmq_channel)
            .await
            .context("Failed to init ha_sync")?;

        // Connect to postgres
        let db = Arc::new(
            DbInterface::connect(&settings.database).context("Failed to connect to database")?,
        );

        // Discover OIDC Provider
        let oidc = Arc::new(
            OidcContext::from_config(settings.oidc.clone())
                .await
                .context("Failed to initialize OIDC Context")?,
        );

        // Build redis client. Does not check if redis is reachable.
        let redis = redis::Client::open(settings.redis.url.clone()).context("Invalid redis url")?;
        let redis_conn = redis::aio::ConnectionManager::new(redis)
            .await
            .context("Failed to create redis connection manager")?;

        let (shutdown, _) = broadcast::channel::<()>(1);
        let (reload, _) = broadcast::channel::<()>(4);

        let mut signaling = SignalingModules::default();

        // Add default modules
        signaling.add_module::<BreakoutRooms>(());

        Ok(Self {
            startup_settings: settings,
            shared_settings,
            args,
            db,
            oidc,
            rabbitmq,
            rabbitmq_channel,
            redis: redis_conn,
            shutdown,
            reload,
            signaling,
        })
    }

    /// Runs the controller until a fatal error occurred or a shutdown is requested (e.g. SIGTERM).
    pub async fn run(self) -> Result<()> {
        // Start internal HTTP Server
        let int_http_server = {
            let db_ctx = Arc::downgrade(&self.db);
            let oidc_ctx = Arc::downgrade(&self.oidc);
            let shutdown = self.shutdown.clone();

            HttpServer::new(move || {
                // Unwraps cannot panic. Server gets stopped before dropping the Arc.
                let db_ctx = Data::from(db_ctx.upgrade().unwrap());
                let oidc_ctx = Data::from(oidc_ctx.upgrade().unwrap());

                App::new()
                    .wrap(TracingLogger::<ReducedSpanBuilder>::new())
                    .app_data(db_ctx)
                    .app_data(oidc_ctx)
                    .app_data(Data::new(shutdown.clone()))
                    .service(api::internal::introspect)
            })
        };

        let signaling_modules = Arc::new(self.signaling);

        // Start external HTTP Server
        let ext_http_server = {
            let cors = self.startup_settings.http.cors.clone();

            let rabbitmq_channel = Data::new(self.rabbitmq_channel);

            let signaling_modules = Arc::downgrade(&signaling_modules);
            let db_ctx = Arc::downgrade(&self.db);
            let oidc_ctx = Arc::downgrade(&self.oidc);
            let shutdown = self.shutdown.clone();
            let shared_settings = self.shared_settings.clone();
            let redis = self.redis;

            HttpServer::new(move || {
                let cors = setup_cors(&cors);

                // Unwraps cannot panic. Server gets stopped before dropping the Arc.
                let db_ctx = Data::from(db_ctx.upgrade().unwrap());
                let oidc_ctx = Data::from(oidc_ctx.upgrade().unwrap());
                let redis = Data::new(redis.clone());

                let signaling_modules = Data::from(signaling_modules.upgrade().unwrap());

                App::new()
                    .wrap(TracingLogger::<ReducedSpanBuilder>::new())
                    .wrap(cors)
                    .app_data(Data::from(shared_settings.clone()))
                    .app_data(db_ctx.clone())
                    .app_data(oidc_ctx.clone())
                    .app_data(redis)
                    .app_data(Data::new(shutdown.clone()))
                    .app_data(rabbitmq_channel.clone())
                    .app_data(signaling_modules)
                    .app_data(SignalingProtocols::data())
                    .service(api::signaling::ws_service)
                    .service(v1_scope(db_ctx, oidc_ctx))
            })
        };

        let ext_address = (Ipv6Addr::UNSPECIFIED, self.startup_settings.http.port);
        let int_address = (
            Ipv6Addr::UNSPECIFIED,
            self.startup_settings.http.internal_port,
        );

        let (ext_http_server, int_http_server) = if let Some(tls) = &self.startup_settings.http.tls
        {
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

        // Select over both http servers a SIGTERM event and SIGHUP event
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

                    if let Err(e) = settings::reload_settings(self.shared_settings.clone(), &self.args.config) {
                        log::error!("Failed to reload settings, {}", e);
                        continue
                    }

                    // discard result, might fail if no one is subscribed
                    let _ = self.reload.send(());
                }
            }
        }

        // ==== Begin shutdown sequence ====

        // Send shutdown signals to all tasks within our application
        let _ = self.shutdown.send(());

        // then stop HTTP servers
        ext_server.stop(true).await;
        int_server.stop(true).await;

        // Check in a 1 second interval for 10 seconds if all tasks have exited
        // by inspecting the receiver count of the broadcast-channel
        for _ in 0..10 {
            let receiver_count = self.shutdown.receiver_count();

            if receiver_count > 0 {
                log::debug!("Waiting for {} tasks to be stopped", receiver_count);
                sleep(Duration::from_secs(1)).await;
            }
        }

        // Drop signaling modules to drop any data contained in the module builders.
        drop(signaling_modules);

        // Close all rabbitmq connections
        // TODO what code and text to use here
        if let Err(e) = self.rabbitmq.close(0, "shutting down").await {
            log::error!("Failed to close RabbitMQ connections, {}", e);
        }

        if self.shutdown.receiver_count() > 0 {
            log::error!("Not all tasks stopped. Exiting anyway");
        } else {
            log::info!("All tasks stopped, goodbye!");
        }

        Ok(())
    }
}

fn v1_scope(db_ctx: Data<DbInterface>, oidc_ctx: Data<OidcContext>) -> Scope {
    // the latest version contains the root services
    web::scope("/v1")
        .service(api::v1::auth::login)
        .service(api::v1::auth::oidc_provider)
        .service(api::v1::rooms::start_invited)
        .service(api::v1::invites::verify_invite_code)
        .service(api::v1::turn::get)
        .service(api::v1::rooms::sip_start)
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
                .service(api::v1::sip_configs::get)
                .service(api::v1::sip_configs::put)
                .service(api::v1::sip_configs::delete)
                .service(api::v1::invites::get_invites)
                .service(api::v1::invites::add_invite)
                .service(api::v1::invites::get_invite)
                .service(api::v1::invites::update_invite)
                .service(api::v1::invites::delete_invite),
        )
}

fn setup_cors(settings: &settings::HttpCors) -> Cors {
    let mut cors = Cors::default();

    for origin in &settings.allowed_origin {
        cors = cors.allowed_origin(origin)
    }

    cors.allowed_header(header::CONTENT_TYPE)
        .allowed_header(header::AUTHORIZATION)
        .allow_any_method()
}

fn setup_rustls(tls: &settings::HttpTls) -> Result<rustls::ServerConfig> {
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
