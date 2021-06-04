use crate::modules::http::ws::{Event, EventCtx, WebSocketModule};
use serde_json::Value;
use tokio_stream::Pending;

/// A sample echo websocket module
pub struct Echo;

#[async_trait::async_trait(?Send)]
impl WebSocketModule for Echo {
    const NAMESPACE: &'static str = "echo";
    type Params = ();
    type Incoming = Value;
    type Outgoing = Value;
    type ExtEvent = ();
    type ExtEventStream = Pending<()>;

    async fn init(_: &Self::Params, _: &'static str) -> Self {
        Self
    }

    async fn events(&mut self) -> Option<Self::ExtEventStream> {
        None
    }

    async fn on_event(&mut self, mut ctx: EventCtx<'_, Self>, event: Event<Self>) {
        match event {
            Event::WsMessage(incoming) => {
                ctx.ws_send(incoming);
            }
            Event::Ext(_) => unreachable!("no registered external events"),
        }
    }

    async fn on_destroy(self) {}
}
