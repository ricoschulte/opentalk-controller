//! Test utility functions for use with the module tester and the controller
use crate::database::DatabaseContext;
use anyhow::Result;
use controller::prelude::anyhow::Context;
use controller::prelude::redis::aio::ConnectionManager;
use controller::prelude::*;
use db_storage::rooms::RoomId;
use db_storage::users::UserId;
use uuid::Uuid;

pub mod common;
pub mod database;
pub mod redis;

#[derive(Debug)]
pub struct TestUser {
    pub user_id: UserId,
    pub participant_id: ParticipantId,
    pub name: &'static str,
}

pub const ROOM_ID: RoomId = RoomId::from(Uuid::from_u128(2000));

pub const USER_1: TestUser = TestUser {
    user_id: UserId::from(1),
    participant_id: ParticipantId::new_test(1),
    name: "user1",
};

pub const USER_2: TestUser = TestUser {
    user_id: UserId::from(2),
    participant_id: ParticipantId::new_test(2),
    name: "user2",
};

pub const USERS: [TestUser; 2] = [USER_1, USER_2];

/// The [`TestContext`] provides access to redis & postgres for tests
pub struct TestContext {
    pub db_ctx: DatabaseContext,
    pub redis_conn: ConnectionManager,
}

impl TestContext {
    /// Creates a new [`TestContext`]
    pub async fn new() -> Self {
        let _ = setup_logging();

        TestContext {
            db_ctx: database::DatabaseContext::new(true).await,
            redis_conn: redis::setup().await,
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
