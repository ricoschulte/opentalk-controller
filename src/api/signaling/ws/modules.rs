use super::runner::Runner;
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

    pub async fn on_event_targeted(
        &mut self,
        ctx: DynEventCtx<'_>,
        module: &str,
        dyn_event: DynTargetedEvent,
    ) -> Result<(), NoSuchModuleError> {
        let module = self.modules.get_mut(module).ok_or(NoSuchModuleError(()))?;

        if let Err(e) = module.on_event_targeted(ctx, dyn_event).await {
            log::error!("Failed to handle event {:?}", e);
        }

        Ok(())
    }

    pub async fn on_event_broadcast(
        &mut self,
        ctx: DynEventCtx<'_>,
        dyn_event: DynBroadcastEvent<'_>,
    ) {
        for module in self.modules.values_mut() {
            let ctx = DynEventCtx {
                id: ctx.id,
                ws_messages: ctx.ws_messages,
                rabbitmq_messages: ctx.rabbitmq_messages,
                events: ctx.events,
                storage: ctx.storage,
                invalidate_data: ctx.invalidate_data,
            };

            if let Err(e) = module.on_event_broadcast(ctx, dyn_event).await {
                log::error!("Failed to handle event, {:?}", e);
            }
        }
    }

    pub async fn collect_participant_data(
        &self,
        storage: &mut Storage,
        participant: &mut Participant,
    ) -> Result<()> {
        for module in self.modules.values() {
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

#[derive(Debug)]
pub enum DynTargetedEvent {
    WsMessage(Value),
    RabbitMqMessage(Value),
    Ext(Box<dyn Any + 'static>),
}

#[derive(Debug, Copy, Clone)]
pub enum DynBroadcastEvent<'evt> {
    ParticipantJoined(&'evt Participant),
    ParticipantLeft(ParticipantId),
    ParticipantUpdated(&'evt Participant),
}

pub struct DynEventCtx<'ctx> {
    pub id: ParticipantId,
    pub ws_messages: &'ctx mut Vec<Message>,
    pub rabbitmq_messages: &'ctx mut Vec<(Option<ParticipantId>, String)>,
    pub events: &'ctx mut SelectAll<AnyStream>,
    pub storage: &'ctx mut Storage,
    pub invalidate_data: &'ctx mut bool,
}

#[async_trait::async_trait(?Send)]
trait ModuleCaller {
    async fn on_event_targeted(
        &mut self,
        ctx: DynEventCtx<'_>,
        dyn_event: DynTargetedEvent,
    ) -> Result<()>;
    async fn on_event_broadcast(
        &mut self,
        ctx: DynEventCtx<'_>,
        dyn_event: DynBroadcastEvent<'_>,
    ) -> Result<()>;
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
    async fn on_event_targeted(
        &mut self,
        ctx: DynEventCtx<'_>,
        dyn_event: DynTargetedEvent,
    ) -> Result<()> {
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
            DynTargetedEvent::WsMessage(msg) => {
                let msg = serde_json::from_value(msg).context("Failed to parse message")?;
                self.module.on_event(ctx, Event::WsMessage(msg)).await
            }
            DynTargetedEvent::RabbitMqMessage(msg) => {
                let msg = serde_json::from_value(msg).context("Failed to parse message")?;
                self.module.on_event(ctx, Event::RabbitMq(msg)).await
            }
            DynTargetedEvent::Ext(ext) => {
                self.module
                    .on_event(ctx, Event::Ext(*ext.downcast().expect("invalid ext type")))
                    .await
            }
        }
    }

    async fn on_event_broadcast(
        &mut self,
        ctx: DynEventCtx<'_>,
        dyn_event: DynBroadcastEvent<'_>,
    ) -> Result<()> {
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
            DynBroadcastEvent::ParticipantJoined(participant) => {
                if let Some(module_data) = participant.module_data.get(M::NAMESPACE) {
                    let module_data = serde_json::from_value(module_data.clone())
                        .context("Failed to parse module data")?;

                    self.module
                        .on_event(ctx, Event::ParticipantJoined(participant.id, module_data))
                        .await?;
                }
            }
            DynBroadcastEvent::ParticipantLeft(participant) => {
                self.module
                    .on_event(ctx, Event::ParticipantLeft(participant))
                    .await?;
            }
            DynBroadcastEvent::ParticipantUpdated(participant) => {
                if let Some(module_data) = participant.module_data.get(M::NAMESPACE) {
                    let module_data = serde_json::from_value(module_data.clone())
                        .context("Failed to parse module data")?;

                    self.module
                        .on_event(ctx, Event::ParticipantUpdated(participant.id, module_data))
                        .await?;
                }
            }
        }

        Ok(())
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
    async fn build(&self, runner: &mut Runner) -> Result<()>;

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
    async fn build(&self, runner: &mut Runner) -> Result<()> {
        let ctx = ModuleContext {
            id: runner.id,
            ws_messages: &mut vec![],
            rabbitmq_messages: &mut vec![],
            events: &mut runner.events,
            storage: &mut runner.storage,
            invalidate_data: &mut false,
            m: PhantomData::<fn() -> M>,
        };

        let module = M::init(ctx, &self.params, runner.protocol).await?;

        runner.modules.add_module(module).await;

        Ok(())
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
