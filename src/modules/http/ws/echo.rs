use crate::modules::http::ws::{Event, WebSocketModule, WsCtx};
use serde_json::Value;

/// A sample echo websocket module
pub struct Echo;

#[async_trait::async_trait(?Send)]
impl WebSocketModule for Echo {
    const NAMESPACE: &'static str = "echo";
    type Params = ();
    type Incoming = Value;
    type Outgoing = Value;
    type ExtEvent = ();

    async fn init(_: WsCtx<'_, Self>, _: &Self::Params, _: &'static str) -> Self {
        Self
    }

    async fn on_event(&mut self, mut ctx: WsCtx<'_, Self>, event: Event<Self>) {
        match event {
            Event::WsMessage(incoming) => {
                ctx.ws_send(incoming);
            }
            Event::Ext(_) => unreachable!("no registered external events"),
        }
    }

    async fn on_destroy(self) {}
}
