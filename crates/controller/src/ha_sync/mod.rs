//! High availability controller synchronization events

use anyhow::{Context, Result};
use lapin::ExchangeKind;

pub mod user_update;

pub async fn init(rabbitmq_channel: &lapin::Channel) -> Result<()> {
    rabbitmq_channel
        .exchange_declare(
            user_update::EXCHANGE,
            ExchangeKind::Topic,
            Default::default(),
            Default::default(),
        )
        .await
        .context("Failed to declare user-update exchange")?;

    Ok(())
}
