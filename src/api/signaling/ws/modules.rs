use super::SignalingModule;
use super::{Event, ModuleContext};
use crate::api::signaling::storage::Storage;
use crate::api::signaling::ws_modules::control::outgoing::Participant;
use crate::api::signaling::ParticipantId;
use anyhow::{Context, Result};
use async_tungstenite::tungstenite::Message;
use futures::stream::SelectAll;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::pin::Pin;
use tokio_stream::{Stream, StreamExt};

pub type AnyStream = Pin<Box<dyn Stream<Item = (&'static str, Box<dyn Any + 'static>)>>>;

pub fn any_stream<S>(namespace: &'static str, stream: S) -> AnyStream
where
    S: Stream + 'static,
{
    Box::pin(stream.map(move |item| -> (_, Box<dyn Any + 'static>) { (namespace, Box::new(item)) }))
}

#[derive(Debug, thiserror::Error)]
#[error("invalid module namespace")]
pub struct NoSuchModuleError(pub ());

#[derive(Default)]
pub struct Modules {
    modules: HashMap<&'static str, Box<dyn ModuleCaller>>,
    events: SelectAll<AnyStream>,
}

impl Modules {
    pub async fn add_module<M>(&mut self, module: M)
    where
        M: SignalingModule,
    {
        log::debug!("Registering module {}", M::NAMESPACE);

        self.modules
            .insert(M::NAMESPACE, Box::new(ModuleCallerImpl { module }));
    }

    pub async fn poll_ext_events(&mut self) -> Option<(&'static str, DynEvent)> {
        let (module, event) = self.events.next().await?;
        Some((module, DynEvent::Ext(event)))
    }

    pub async fn on_event(
        &mut self,
        id: ParticipantId,
        ws_messages: &mut Vec<Message>,
        rabbitmq_messages: &mut Vec<(Option<ParticipantId>, String)>,
        storage: &mut Storage,
        invalidate_data: &mut bool,
        module: &str,
        dyn_event: DynEvent,
    ) -> Result<(), NoSuchModuleError> {
        let module = self.modules.get_mut(module).ok_or(NoSuchModuleError(()))?;

        let ctx = DynEventCtx {
            id,
            ws_messages,
            rabbitmq_messages,
            events: &mut self.events,
            storage,
            invalidate_data,
        };

        module.on_event(ctx, dyn_event).await;

        Ok(())
    }

    pub async fn collect_participant_data(
        &self,
        storage: &mut Storage,
        participant: &mut Participant,
    ) -> Result<()> {
        for (_, module) in &self.modules {
            module
                .populate_frontend_data_for(storage, participant)
                .await?;
        }

        Ok(())
    }

    pub async fn destroy(self, storage: &mut Storage) {
        for (namespace, module) in self.modules {
            log::debug!("Destroying module {}", namespace);

            module.destroy(storage).await;
        }
    }
}

pub enum DynEvent {
    WsMessage(Value),
    RabbitMqMessage(Value),
    Ext(Box<dyn Any + 'static>),
}

struct DynEventCtx<'ctx> {
    id: ParticipantId,
    ws_messages: &'ctx mut Vec<Message>,
    rabbitmq_messages: &'ctx mut Vec<(Option<ParticipantId>, String)>,
    events: &'ctx mut SelectAll<AnyStream>,
    storage: &'ctx mut Storage,
    invalidate_data: &'ctx mut bool,
}

#[async_trait::async_trait(?Send)]
trait ModuleCaller {
    async fn on_event(&mut self, ctx: DynEventCtx<'_>, dyn_event: DynEvent);
    async fn populate_frontend_data_for(
        &self,
        storage: &mut Storage,
        participant: &mut Participant,
    ) -> Result<()>;
    async fn destroy(self: Box<Self>, storage: &mut Storage);
}

struct ModuleCallerImpl<M> {
    pub module: M,
}

#[async_trait::async_trait(?Send)]
impl<M> ModuleCaller for ModuleCallerImpl<M>
where
    M: SignalingModule,
{
    async fn on_event(&mut self, ctx: DynEventCtx<'_>, dyn_event: DynEvent) {
        let ctx = ModuleContext {
            id: ctx.id,
            ws_messages: ctx.ws_messages,
            rabbitmq_messages: ctx.rabbitmq_messages,
            events: ctx.events,
            storage: ctx.storage,
            invalidate_data: ctx.invalidate_data,
            m: PhantomData::<fn() -> M>,
        };

        match dyn_event {
            DynEvent::WsMessage(msg) => match serde_json::from_value(msg) {
                Ok(msg) => self.module.on_event(ctx, Event::WsMessage(msg)).await,
                Err(e) => log::error!("Unable to parse message, {}", e),
            },
            DynEvent::RabbitMqMessage(msg) => match serde_json::from_value(msg) {
                Ok(msg) => self.module.on_event(ctx, Event::RabbitMq(msg)).await,
                Err(e) => log::error!("Failed to parse incoming rabbitmq message, {}", e),
            },
            DynEvent::Ext(ext) => {
                self.module
                    .on_event(ctx, Event::Ext(*ext.downcast().expect("invalid ext type")))
                    .await;
            }
        }
    }

    async fn populate_frontend_data_for(
        &self,
        storage: &mut Storage,
        participant: &mut Participant,
    ) -> Result<()> {
        let data = self
            .module
            .get_frontend_data_for(storage, participant.id)
            .await?;

        let value =
            serde_json::to_value(&data).context("Failed to convert FrontendData to json")?;

        if let Value::Null = &value {
            return Ok(());
        }

        participant.module_data.insert(M::NAMESPACE.into(), value);

        Ok(())
    }

    async fn destroy(self: Box<Self>, storage: &mut Storage) {
        self.module.on_destroy(storage).await
    }
}

#[async_trait::async_trait(?Send)]
pub trait ModuleBuilder: Send + Sync {
    async fn build(
        &self,
        id: ParticipantId,
        builder: &mut Modules,
        storage: &mut Storage,
        protocol: &'static str,
    );

    fn clone_boxed(&self) -> Box<dyn ModuleBuilder>;
}

pub struct ModuleBuilderImpl<M>
where
    M: SignalingModule,
{
    pub m: PhantomData<fn() -> M>,
    pub params: M::Params,
}

#[async_trait::async_trait(?Send)]
impl<M> ModuleBuilder for ModuleBuilderImpl<M>
where
    M: SignalingModule,
{
    async fn build(
        &self,
        id: ParticipantId,
        builder: &mut Modules,
        storage: &mut Storage,
        protocol: &'static str,
    ) {
        let ctx = ModuleContext {
            id,
            ws_messages: &mut vec![],
            rabbitmq_messages: &mut vec![],
            events: &mut builder.events,
            storage,
            invalidate_data: &mut false,
            m: PhantomData::<fn() -> M>,
        };

        let module = M::init(ctx, &self.params, protocol).await;

        builder.add_module(module).await
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
