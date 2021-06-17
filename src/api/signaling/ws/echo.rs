use crate::api::signaling::storage::Storage;
use crate::api::signaling::ws::{Event, ModuleContext, SignalingModule};
use crate::api::signaling::ParticipantId;
use anyhow::Result;
use serde_json::Value;

/// A sample echo websocket module
pub struct Echo;

#[async_trait::async_trait(?Send)]
impl SignalingModule for Echo {
    const NAMESPACE: &'static str = "echo";
    type Params = ();
    type Incoming = Value;
    type Outgoing = Value;
    type RabbitMqMessage = ();
    type ExtEvent = ();
    type FrontendData = ();
    type PeerFrontendData = ();

    async fn init(_: ModuleContext<'_, Self>, _: &Self::Params, _: &'static str) -> Self {
        Self
    }

    async fn on_event(&mut self, mut ctx: ModuleContext<'_, Self>, event: Event<Self>) {
        match event {
            Event::WsMessage(incoming) => {
                ctx.ws_send(incoming);
            }
            Event::RabbitMq(msg) => {
                ctx.rabbitmq_send(None, msg);
            }
            Event::Ext(_) => unreachable!("no registered external events"),
            // Ignore
            Event::ParticipantJoined(_) => {}
            Event::ParticipantLeft(_) => {}
            Event::ParticipantUpdated(_) => {}
        }
    }

    async fn get_frontend_data(&self) -> Self::FrontendData {
        ()
    }

    async fn get_frontend_data_for(
        &self,
        _: &mut Storage,
        _: ParticipantId,
    ) -> Result<Self::FrontendData> {
        Ok(())
    }

    async fn on_destroy(self, _: &mut Storage) {}
}
