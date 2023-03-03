// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

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
//!    if let Some(controller) = Controller::create("K3K Controller Community Edition").await? {
//!         controller.run().await?;
//!     }
//!
//!     Ok(())
//! }
//! ```

use crate::acl::check_or_create_kustos_default_permissions;
use crate::api::v1::middleware::metrics::RequestMetrics;
use crate::api::v1::response::error::json_error_handler;
use crate::services::MailService;
use crate::settings::{Settings, SharedSettings};
use crate::trace::ReducedSpanBuilder;
use actix_cors::Cors;
use actix_web::http::header;
use actix_web::web::Data;
use actix_web::{web, App, HttpServer, Scope};
use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwap;
use breakout::BreakoutRooms;
use database::Db;
use keycloak_admin::KeycloakAdminClient;
use lapin_pool::{RabbitMqChannel, RabbitMqPool};
use moderation::ModerationModule;
use oidc::OidcContext;
use prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;
use storage::ObjectStorage;
use tokio::signal::ctrl_c;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::time::sleep;
use tracing_actix_web::TracingLogger;

#[cfg(not(doc))]
mod api;
#[cfg(doc)]
pub mod api;

mod acl;
mod cli;
mod ha_sync;
mod metrics;
mod oidc;
mod redis_wrapper;
pub mod storage;
mod trace;

mod services;
pub mod settings;

pub mod prelude {
    pub use crate::api::signaling::prelude::*;
    pub use crate::api::Participant;
    pub use crate::redis_wrapper::RedisConnection;

    // re-export commonly used crates to reduce dependency management in module-crates
    pub use actix_web;
    pub use anyhow;
    pub use async_trait;
    pub use aws_sdk_s3;
    pub use bytes;
    pub use chrono;
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

#[derive(Debug, thiserror::Error)]
#[error("Blocking thread has panicked")]
pub struct BlockingError;

/// Custom version of `actix_web::web::block` which retains the current tracing span
pub async fn block<F, R>(f: F) -> Result<R, BlockingError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();

    let fut = actix_rt::task::spawn_blocking(move || span.in_scope(f));

    fut.await.map_err(|_| BlockingError)
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
                eprintln!("Crashed with error: {err:?}");
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

    /// CLI arguments
    args: cli::Args,

    db: Arc<Db>,

    storage: Arc<ObjectStorage>,

    oidc: Arc<OidcContext>,

    kc_admin_client: Arc<KeycloakAdminClient>,

    /// RabbitMQ connection pool, can be used to create connections and channels
    pub rabbitmq_pool: Arc<RabbitMqPool>,

    /// General purpose rabbitmq channel
    pub rabbitmq_channel: Arc<RabbitMqChannel>,

    /// Cloneable redis connection manager, can be used to write/read to the controller's redis.
    pub redis: RedisConnection,

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

    /// All metrics of the Application
    pub metrics: metrics::CombinedMetrics,
}

impl Controller {
    /// Tries to create a controller from CLI arguments and then the settings.
    ///
    /// This can return Ok(None) which would indicate that the controller executed a CLI
    /// subprogram (e.g. `--reload`) and must now exit.
    ///
    /// Otherwise it will return itself which can be modified and then run using [`Controller::run`]
    pub async fn create(program_name: &str) -> Result<Option<Self>> {
        let args = cli::parse_args().await?;

        // Some args run commands by them self and thus should exit here
        if !args.controller_should_start() {
            return Ok(None);
        }

        let settings = settings::load_settings(&args)?;

        trace::init(&settings.logging)?;

        log::info!("Starting {}", program_name);

        let controller = Self::init(settings, args).await?;

        Ok(Some(controller))
    }

    #[tracing::instrument(err, skip(settings, args))]
    async fn init(settings: Settings, args: cli::Args) -> Result<Self> {
        let settings = Arc::new(settings);
        let shared_settings: SharedSettings = Arc::new(ArcSwap::from(settings.clone()));

        let metrics = metrics::CombinedMetrics::init();

        db_storage::migrations::migrate_from_url(&settings.database.url)
            .await
            .context("Failed to migrate database")?;

        let rabbitmq_pool = RabbitMqPool::from_config(
            &settings.rabbit_mq.url,
            settings.rabbit_mq.min_connections,
            settings.rabbit_mq.max_channels_per_connection,
        );
        // create a general purpose rabbitmq channel for endpoints
        let rabbitmq_channel = Arc::new(
            rabbitmq_pool
                .create_channel()
                .await
                .context("Could not create rabbitmq channel")?,
        );

        ha_sync::init(&rabbitmq_channel)
            .await
            .context("Failed to init ha_sync")?;

        // Connect to postgres
        let mut db = Db::connect(&settings.database).context("Failed to connect to database")?;
        db.set_metrics(metrics.database.clone());
        let db = Arc::new(db);

        // Connect to MinIO
        let storage = Arc::new(ObjectStorage::new(&settings.minio).await?);

        // Discover OIDC Provider
        let oidc = Arc::new(
            OidcContext::from_config(settings.keycloak.clone())
                .await
                .context("Failed to initialize OIDC Context")?,
        );

        let kc_admin_client = Arc::new(KeycloakAdminClient::new(
            settings.keycloak.base_url.clone(),
            settings.keycloak.realm.clone(),
            settings.keycloak.client_id.clone().into(),
            settings.keycloak.client_secret.secret().clone(),
        )?);

        // Build redis client. Does not check if redis is reachable.
        let redis = redis::Client::open(settings.redis.url.clone()).context("Invalid redis url")?;
        let redis_conn = redis::aio::ConnectionManager::new(redis)
            .await
            .context("Failed to create redis connection manager")?;
        let redis_conn = RedisConnection::new(redis_conn).with_metrics(metrics.redis.clone());

        let (shutdown, _) = broadcast::channel::<()>(1);
        let (reload, _) = broadcast::channel::<()>(4);

        let mut signaling = SignalingModules::default();

        // Add default modules
        signaling.add_module::<BreakoutRooms>(());
        signaling.add_module::<ModerationModule>(());
        if let Some(queue) = settings.rabbit_mq.recording_task_queue.clone() {
            signaling.add_module::<recording::Recording>(recording::RecordingParams { queue });
        }

        Ok(Self {
            startup_settings: settings,
            shared_settings,
            args,
            db,
            storage,
            oidc,
            kc_admin_client,
            rabbitmq_pool,
            rabbitmq_channel,
            redis: redis_conn,
            shutdown,
            reload,
            signaling,
            metrics,
        })
    }

    /// Runs the controller until a fatal error occurred or a shutdown is requested (e.g. SIGTERM).
    pub async fn run(self) -> Result<()> {
        let signaling_modules = Arc::new(self.signaling);

        // Start HTTP Server
        let http_server = {
            let cors = self.startup_settings.http.cors.clone();

            let rabbitmq_pool = Data::from(self.rabbitmq_pool.clone());
            let rabbitmq_channel = Data::from(self.rabbitmq_channel.clone());
            let signaling_modules = Arc::downgrade(&signaling_modules);
            let signaling_metrics = Data::from(self.metrics.signaling.clone());
            let db = Arc::downgrade(&self.db);
            let storage = Arc::downgrade(&self.storage);

            let oidc_ctx = Arc::downgrade(&self.oidc);
            let shutdown = self.shutdown.clone();
            let shared_settings = self.shared_settings.clone();
            let redis = self.redis;

            let kc_admin_client = Data::from(self.kc_admin_client);

            let mail_service = Data::new(MailService::new(
                self.shared_settings.clone(),
                self.metrics.endpoint.clone(),
                self.rabbitmq_channel,
            ));

            // TODO(r.floren) what to do with the handle
            let (authz, _) = kustos::Authz::new_with_autoload_and_metrics(
                db.upgrade().unwrap(),
                self.shutdown.subscribe(),
                self.startup_settings.authz.reload_interval,
                self.metrics.kustos.clone(),
            )
            .await?;

            log::info!("Making sure the default permissions are set");
            check_or_create_kustos_default_permissions(&authz).await?;

            let authz_middleware = authz.actix_web_middleware(true).await?;

            let metrics = Data::new(self.metrics);

            HttpServer::new(move || {
                let cors = setup_cors(&cors);

                // Unwraps cannot panic. Server gets stopped before dropping the Arc.
                let db = Data::from(db.upgrade().unwrap());
                let storage = Data::from(storage.upgrade().unwrap());

                let oidc_ctx = Data::from(oidc_ctx.upgrade().unwrap());
                let redis = Data::new(redis.clone());
                let authz = Data::new(authz.clone());

                let mail_service = mail_service.clone();

                let acl = authz_middleware.clone();

                let signaling_modules = Data::from(signaling_modules.upgrade().unwrap());

                App::new()
                    .wrap(api::v1::middleware::headers::Headers {})
                    .wrap(TracingLogger::<ReducedSpanBuilder>::new())
                    .wrap(cors)
                    .wrap(RequestMetrics::new(metrics.endpoint.clone()))
                    .app_data(web::JsonConfig::default().error_handler(json_error_handler))
                    .app_data(Data::from(shared_settings.clone()))
                    .app_data(db.clone())
                    .app_data(storage)
                    .app_data(oidc_ctx.clone())
                    .app_data(kc_admin_client.clone())
                    .app_data(authz)
                    .app_data(redis)
                    .app_data(Data::new(shutdown.clone()))
                    .app_data(rabbitmq_pool.clone())
                    .app_data(rabbitmq_channel.clone())
                    .app_data(signaling_modules)
                    .app_data(SignalingProtocols::data())
                    .app_data(signaling_metrics.clone())
                    .app_data(metrics.clone())
                    .app_data(mail_service)
                    .service(api::signaling::ws_service)
                    .service(metrics::metrics)
                    .service(v1_scope(db.clone(), oidc_ctx.clone(), acl))
                    .service(internal_scope(db, oidc_ctx))
            })
        };

        let address = (Ipv6Addr::UNSPECIFIED, self.startup_settings.http.port);

        let http_server = if let Some(tls) = &self.startup_settings.http.tls {
            let config = setup_rustls(tls).context("Failed to setup TLS context")?;

            http_server.bind_rustls(address, config)
        } else {
            http_server.bind(address)
        };

        let http_server = http_server.with_context(|| {
            format!("Failed to bind http server to {}:{}", address.0, address.1)
        })?;

        log::info!("Startup finished");

        let http_server = http_server.disable_signals().run();
        let http_server_handle = http_server.handle();

        let mut reload_signal =
            signal(SignalKind::hangup()).context("Failed to register SIGHUP signal handler")?;

        actix_rt::spawn(http_server);

        // Wait for either SIGTERM or SIGHUP and handle them accordingly
        loop {
            tokio::select! {
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

        // then stop HTTP server
        http_server_handle.stop(true).await;

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
        if let Err(e) = self.rabbitmq_pool.close(0, "shutting down").await {
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

fn v1_scope(
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
    acl: kustos::actix_web::KustosService,
) -> Scope {
    // the latest version contains the root services
    web::scope("/v1")
        .service(api::v1::auth::login)
        .service(api::v1::auth::oidc_provider)
        .service(api::v1::rooms::start_invited)
        .service(api::v1::invites::verify_invite_code)
        .service(api::v1::turn::get)
        .service(
            web::scope("/services")
                .wrap(api::v1::middleware::service_auth::ServiceAuth::new(
                    oidc_ctx.clone(),
                ))
                .service(api::v1::services::call_in::services())
                .service(api::v1::services::recording::services()),
        )
        .service(
            // empty scope to differentiate between auth endpoints
            web::scope("")
                .wrap(acl)
                .wrap(api::v1::middleware::user_auth::OidcAuth { db, oidc_ctx })
                .service(api::v1::users::find)
                .service(api::v1::users::patch_me)
                .service(api::v1::users::get_me)
                .service(api::v1::users::get_me_tariff)
                .service(api::v1::users::get_user)
                .service(api::v1::rooms::accessible)
                .service(api::v1::rooms::new)
                .service(api::v1::rooms::patch)
                .service(api::v1::rooms::get)
                .service(api::v1::rooms::get_room_tariff)
                .service(api::v1::rooms::start)
                .service(api::v1::rooms::delete)
                .service(api::v1::legal_vote::get_all)
                .service(api::v1::legal_vote::get_all_for_room)
                .service(api::v1::legal_vote::get_specific)
                .service(api::v1::events::new_event)
                .service(api::v1::events::get_events)
                .service(api::v1::events::get_event)
                .service(api::v1::events::patch_event)
                .service(api::v1::events::delete_event)
                .service(api::v1::events::favorites::add_event_to_favorites)
                .service(api::v1::events::favorites::remove_event_from_favorites)
                .service(api::v1::events::instances::get_event_instance)
                .service(api::v1::events::instances::get_event_instances)
                .service(api::v1::events::instances::patch_event_instance)
                .service(api::v1::events::invites::create_invite_to_event)
                .service(api::v1::events::invites::get_invites_for_event)
                .service(api::v1::events::invites::delete_invite_to_event)
                .service(api::v1::events::invites::accept_event_invite)
                .service(api::v1::events::invites::decline_event_invite)
                .service(api::v1::sip_configs::get)
                .service(api::v1::sip_configs::put)
                .service(api::v1::sip_configs::delete)
                .service(api::v1::invites::get_invites)
                .service(api::v1::invites::add_invite)
                .service(api::v1::invites::get_invite)
                .service(api::v1::invites::update_invite)
                .service(api::v1::invites::delete_invite)
                .service(api::v1::assets::room_assets)
                .service(api::v1::assets::room_asset)
                .service(api::v1::assets::delete),
        )
}

fn internal_scope(db: Data<Db>, oidc_ctx: Data<OidcContext>) -> Scope {
    // internal apis
    web::scope("/internal").service(
        web::scope("")
            .wrap(api::v1::middleware::user_auth::OidcAuth { db, oidc_ctx })
            .service(api::internal::rooms::delete),
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
    let cert_file = File::open(&tls.certificate)
        .with_context(|| format!("Failed to open certificate file {:?}", &tls.certificate))?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .map_err(|_| anyhow!("Invalid certificate"))?;
    let certs = certs.into_iter().map(rustls::Certificate).collect();

    let private_key_file = File::open(&tls.private_key).with_context(|| {
        format!(
            "Failed to open pkcs8 private key file {:?}",
            &tls.private_key
        )
    })?;
    let mut key = rustls_pemfile::rsa_private_keys(&mut BufReader::new(private_key_file))
        .map_err(|_| anyhow!("Invalid pkcs8 private key"))?;

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, rustls::PrivateKey(key.remove(0)))?;

    Ok(config)
}
