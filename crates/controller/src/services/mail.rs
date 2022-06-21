//! MailService
//!
//! Used to have a clean interface for various kinds of mails
//! that are sent from the Web-API and possibly other connected services.
//!
// TODO We probably can avoid the conversion to MailTasks if no rabbit_mq_queue is set in all mail fns
use anyhow::{Context, Result};
use controller_shared::settings::{Settings, SharedSettings};
use db_storage::{events::Event, rooms::Room, sip_configs::SipConfig, users::User};
use lapin_pool::RabbitMqChannel;
use mail_worker_proto::*;
use std::sync::Arc;

fn to_event(
    event: Event,
    room: Room,
    sip: Option<SipConfig>,
    settings: &Settings,
) -> mail_worker_proto::v1::Event {
    let start_time: Option<v1::Time> = event.starts_at.zip(event.starts_at_tz).map(Into::into);

    let end_time: Option<v1::Time> = event.ends_at.zip(event.ends_at_tz).map(Into::into);

    let call_in =
        if let Some((call_in_settings, sip_config)) = settings.call_in.as_ref().zip(sip.as_ref()) {
            Some(v1::CallIn {
                sip_tel: call_in_settings.tel.clone(),
                sip_id: sip_config.sip_id.to_string(),
                sip_password: sip_config.password.to_string(),
            })
        } else {
            None
        };

    mail_worker_proto::v1::Event {
        // Fixme: Once we are down to a single uuid crate again. Diesel holds us back regarding the current version
        id: uuid_v1::Uuid::from_u128(event.id.inner().as_u128()),
        name: event.title,
        description: event.description,
        start_time,
        end_time,
        rrule: event.recurrence_pattern,
        room: v1::Room {
            id: uuid_v1::Uuid::from_u128(room.id.inner().as_u128()),
            password: room.password,
        },
        call_in,
    }
}

#[derive(Clone)]
pub struct MailService {
    settings: SharedSettings,
    rabbit_mq_channel: Arc<RabbitMqChannel>,
}

impl MailService {
    pub fn new(settings: SharedSettings, rabbit_mq_channel: Arc<RabbitMqChannel>) -> Self {
        Self {
            settings,
            rabbit_mq_channel,
        }
    }

    async fn send_to_rabbitmq(&self, mail_task: MailTask) -> Result<()> {
        if let Some(queue_name) = &self.settings.load().rabbit_mq.mail_task_queue {
            self.rabbit_mq_channel
                .basic_publish(
                    "",
                    queue_name,
                    Default::default(),
                    &serde_json::to_vec(&mail_task).context("Failed to serialize mail_task")?,
                    Default::default(),
                )
                .await?;
        }

        Ok(())
    }

    /// Sends a Registered Invite mail task to the rabbit mq queue, if configured.
    pub async fn send_registered_invite(
        &self,
        inviter: User,
        event: Event,
        room: Room,
        sip_config: Option<SipConfig>,
        invitee: User,
    ) -> Result<()> {
        let settings = &*self.settings.load();

        // Create MailTask
        let mail_task = MailTask::registered_invite(
            inviter,
            to_event(event, room, sip_config, settings),
            invitee,
        );

        self.send_to_rabbitmq(mail_task).await?;
        Ok(())
    }

    /// Sends a Unregistered Invite mail task to the rabbit mq queue, if configured.
    pub async fn send_unregistered_invite(
        &self,
        inviter: User,
        event: Event,
        room: Room,
        sip_config: Option<SipConfig>,
        invitee: &str,
    ) -> Result<()> {
        let settings = &*self.settings.load();

        // Create MailTask
        let mail_task = MailTask::unregistered_invite(
            inviter,
            to_event(event, room, sip_config, settings),
            invitee,
        );

        self.send_to_rabbitmq(mail_task).await?;
        Ok(())
    }
}
