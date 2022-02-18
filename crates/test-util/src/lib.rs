//! Test utility functions for use with the module tester and the controller
use crate::database::DatabaseContext;
use anyhow::Result;
use controller::prelude::anyhow::Context;
use controller::prelude::redis::aio::ConnectionManager;
use controller::prelude::*;
use controller_shared::ParticipantId;
use db_storage::rooms::RoomId;

use kustos::Authz;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use uuid::Uuid;

pub mod common;
pub mod database;
pub mod redis;

#[derive(Debug)]
pub struct TestUser {
    pub n: u32,
    pub participant_id: ParticipantId,
    pub name: &'static str,
}

pub const ROOM_ID: RoomId = RoomId::from(Uuid::from_u128(2000));

pub const USER_1: TestUser = TestUser {
    n: 1,
    participant_id: ParticipantId::new_test(1),
    name: "user1",
};

pub const USER_2: TestUser = TestUser {
    n: 2,
    participant_id: ParticipantId::new_test(2),
    name: "user2",
};

pub const USERS: [TestUser; 2] = [USER_1, USER_2];

/// The [`TestContext`] provides access to redis & postgres for tests
pub struct TestContext {
    pub db_ctx: DatabaseContext,
    pub redis_conn: ConnectionManager,
    pub authz: Arc<Authz>,
    pub shutdown: Sender<()>,
}

impl TestContext {
    /// Creates a new [`TestContext`]
    pub async fn new() -> Self {
        let _ = setup_logging();

        let db_ctx = database::DatabaseContext::new(true).await;

        let (shutdown, _) = tokio::sync::broadcast::channel(10);

        let (enforcer, _) = kustos::Authz::new_with_autoload(
            db_ctx.db_conn.clone(),
            shutdown.subscribe(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        TestContext {
            db_ctx,
            redis_conn: redis::setup().await,
            authz: Arc::new(enforcer),
            shutdown,
        }
    }
}

pub fn setup_logging() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()
        .context("Failed to setup logging utility")
}
