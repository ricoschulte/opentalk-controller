//! High availability controller synchronization events

use anyhow::{Context, Result};
use lapin::ExchangeKind;
use lapin_pool::RabbitMqChannel;

pub mod user_update;

pub async fn init(rabbitmq_channel: &RabbitMqChannel) -> Result<()> {
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
