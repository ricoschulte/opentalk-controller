//! RabbitMQ utilities related to updates of users

use anyhow::{Context, Result};
use db_storage::users::UserId;
use serde::{Deserialize, Serialize};

/// RabbitMQ exchange to send the messages to
pub const EXCHANGE: &str = "user-update";

/// Helper function to generate topic for a update message
pub fn routing_key(user_id: UserId) -> String {
    format!("user.{}", user_id)
}

/// The message sent on an update event
#[derive(Debug, Deserialize, Serialize)]
pub struct Message {
    /// true if groups have been updated.
    /// permission sensitive tasks must recalculate the users permissions
    pub groups: bool,
}

impl Message {
    pub async fn send_via(self, rabbitmq_channel: &lapin::Channel, user_id: UserId) -> Result<()> {
        let message = serde_json::to_vec(&self).context("Failed to serialize message")?;

        rabbitmq_channel
            .basic_publish(
                EXCHANGE,
                &routing_key(user_id),
                Default::default(),
                &message,
                Default::default(),
            )
            .await
            .context("Failed to publish user-update message")?;

        Ok(())
    }
}
