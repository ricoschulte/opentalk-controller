use super::WebSocketModule;
use super::{Event, EventCtx};
use actix_web::dev::Extensions;
use async_tungstenite::tungstenite::Message;
use futures::stream::SelectAll;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::{Stream, StreamExt};

type AnyStream = Pin<Box<dyn Stream<Item = (&'static str, Box<dyn Any + 'static>)>>>;

#[derive(Debug, thiserror::Error)]
#[error("invalid module namespace")]
pub struct NoSuchModuleError(pub ());

#[derive(Default)]
pub struct Modules {
    modules: HashMap<&'static str, Box<dyn ModuleCaller>>,
    events: SelectAll<AnyStream>,
}

impl Modules {
    pub async fn add_module<M>(&mut self, mut module: M)
    where
        M: WebSocketModule,
    {
        log::debug!("Registering module {}", M::NAMESPACE);

        if let Some(events) = module.events().await {
            self.events.push(Box::pin(events.map(
                |item| -> (_, Box<dyn Any + 'static>) { (M::NAMESPACE, Box::new(item)) },
            )));
        }

        self.modules
            .insert(M::NAMESPACE, Box::new(ModuleCallerImpl { module }));
    }

    pub async fn poll_ext_events(&mut self) -> Option<(&'static str, DynEvent)> {
        let (module, event) = self.events.next().await?;
        Some((module, DynEvent::Ext(event)))
    }

    pub async fn on_event(
        &mut self,
        ctx: DynEventCtx<'_>,
        module: &str,
        dyn_event: DynEvent,
    ) -> Result<(), NoSuchModuleError> {
        let module = self.modules.get_mut(module).ok_or(NoSuchModuleError(()))?;

        module.on_event(ctx, dyn_event).await;

        Ok(())
    }

    pub async fn destroy(self) {
        for (namespace, module) in self.modules {
            log::debug!("Destroying module {}", namespace);

            module.destroy().await;
        }
    }
}

pub enum DynEvent {
    WsMessage(Value),
    Ext(Box<dyn Any + 'static>),
}

pub struct DynEventCtx<'ctx> {
    pub(crate) ws_messages: &'ctx mut Vec<Message>,
    pub(crate) local_state: &'ctx mut Extensions,
}

#[async_trait::async_trait(?Send)]
trait ModuleCaller {
    async fn on_event(&mut self, ctx: DynEventCtx<'_>, dyn_event: DynEvent);
    async fn destroy(self: Box<Self>);
}

struct ModuleCallerImpl<M> {
    pub module: M,
}

#[async_trait::async_trait(?Send)]
impl<M> ModuleCaller for ModuleCallerImpl<M>
where
    M: WebSocketModule,
{
    async fn on_event(&mut self, ctx: DynEventCtx<'_>, dyn_event: DynEvent) {
        let ctx = EventCtx {
            ws_messages: ctx.ws_messages,
            local_state: ctx.local_state,
            m: PhantomData::<fn() -> M>,
        };

        match dyn_event {
            DynEvent::WsMessage(msg) => match serde_json::from_value(msg) {
                Err(e) => log::error!("Unable to parse message, {}", e),
                Ok(msg) => self.module.on_event(ctx, Event::WsMessage(msg)).await,
            },
            DynEvent::Ext(ext) => {
                self.module
                    .on_event(ctx, Event::Ext(*ext.downcast().expect("invalid ext type")))
                    .await;
            }
        }
    }

    async fn destroy(self: Box<Self>) {
        self.module.on_destroy().await
    }
}

#[async_trait::async_trait(?Send)]
pub trait ModuleBuilder: Send + Sync {
    async fn build(&self, builder: &mut Modules, protocol: &'static str);

    fn clone_boxed(&self) -> Box<dyn ModuleBuilder>;
}

pub struct ModuleBuilderImpl<M>
where
    M: WebSocketModule,
{
    pub m: PhantomData<fn() -> M>,
    pub params: Arc<M::Params>,
}

#[async_trait::async_trait(?Send)]
impl<M> ModuleBuilder for ModuleBuilderImpl<M>
where
    M: WebSocketModule,
{
    async fn build(&self, builder: &mut Modules, protocol: &'static str) {
        builder
            .add_module(M::init(&*self.params, protocol).await)
            .await
    }

    fn clone_boxed(&self) -> Box<dyn ModuleBuilder> {
        Box::new(ModuleBuilderImpl {
            m: self.m,
            params: self.params.clone(),
        })
    }
}

impl Clone for Box<dyn ModuleBuilder> {
    fn clone(&self) -> Self {
        (**self).clone_boxed()
    }
}
