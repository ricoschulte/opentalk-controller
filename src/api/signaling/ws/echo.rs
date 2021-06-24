use crate::api::signaling::ws::{
    DestroyContext, Event, InitContext, ModuleContext, SignalingModule,
};
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

    async fn init(_: InitContext<'_, Self>, _: &Self::Params, _: &'static str) -> Result<Self> {
        Ok(Self)
    }

    async fn on_event(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::WsMessage(incoming) => {
                ctx.ws_send(incoming);
            }
            Event::RabbitMq(_) => {}
            Event::Ext(_) => unreachable!("no registered external events"),
            // Ignore
            Event::Joined { .. } => {}
            Event::ParticipantJoined(..) => {}
            Event::ParticipantLeft(_) => {}
            Event::ParticipantUpdated(..) => {}
        }

        Ok(())
    }

    async fn on_destroy(self, _: DestroyContext<'_>) {}
}
