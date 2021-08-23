use super::SignalingModule;
use super::{Event, ModuleContext};
use crate::api::signaling::ws::runner::Builder;
use crate::api::signaling::ws::{DestroyContext, InitContext, RabbitMqPublish};
use crate::api::signaling::ws_modules::control::outgoing::Participant;
use crate::api::signaling::ParticipantId;
use anyhow::{Context, Result};
use async_tungstenite::tungstenite::Message;
use redis::aio::ConnectionManager;
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
pub(super) struct Modules {
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
        mut dyn_event: DynBroadcastEvent<'_>,
    ) {
        for module in self.modules.values_mut() {
            let ctx = DynEventCtx {
                id: ctx.id,
                ws_messages: ctx.ws_messages,
                rabbitmq_publish: ctx.rabbitmq_publish,
                redis_conn: ctx.redis_conn,
                invalidate_data: ctx.invalidate_data,
            };

            if let Err(e) = module.on_event_broadcast(ctx, &mut dyn_event).await {
                log::error!("Failed to handle event, {:?}", e);
            }
        }
    }

    pub async fn destroy(&mut self, ctx: DestroyContext<'_>) {
        for (namespace, module) in self.modules.drain() {
            log::debug!("Destroying module {}", namespace);

            module
                .destroy(DestroyContext {
                    redis_conn: ctx.redis_conn,
                    destroy_room: ctx.destroy_room,
                })
                .await;
        }
    }
}

/// Events that are specific to a module
#[derive(Debug)]
pub enum DynTargetedEvent {
    WsMessage(Value),
    RabbitMqMessage(Value),
    Ext(Box<dyn Any + 'static>),
}

/// Events that can dispatched to all modules
#[derive(Debug)]
pub enum DynBroadcastEvent<'evt> {
    Joined(
        &'evt mut HashMap<&'static str, Value>,
        &'evt mut Vec<Participant>,
    ),
    Leaving,
    RaiseHand,
    LowerHand,
    ParticipantJoined(&'evt mut Participant),
    ParticipantLeft(ParticipantId),
    ParticipantUpdated(&'evt mut Participant),
}

/// Untyped version of a ModuleContext which is used in `on_event`
pub(super) struct DynEventCtx<'ctx> {
    pub id: ParticipantId,
    pub ws_messages: &'ctx mut Vec<Message>,
    pub rabbitmq_publish: &'ctx mut Vec<RabbitMqPublish>,
    pub redis_conn: &'ctx mut ConnectionManager,
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
        dyn_event: &mut DynBroadcastEvent<'_>,
    ) -> Result<()>;
    async fn destroy(self: Box<Self>, ctx: DestroyContext<'_>);
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
            ws_messages: ctx.ws_messages,
            rabbitmq_publish: ctx.rabbitmq_publish,
            redis_conn: ctx.redis_conn,
            invalidate_data: ctx.invalidate_data,
            m: PhantomData::<fn() -> M>,
        };

        match dyn_event {
            DynTargetedEvent::WsMessage(msg) => {
                let msg = serde_json::from_value(msg).context("Failed to parse WS message")?;
                self.module.on_event(ctx, Event::WsMessage(msg)).await
            }
            DynTargetedEvent::RabbitMqMessage(msg) => {
                let msg =
                    serde_json::from_value(msg).context("Failed to parse RabbitMq message")?;
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
        dyn_event: &mut DynBroadcastEvent<'_>,
    ) -> Result<()> {
        let ctx = ModuleContext {
            ws_messages: ctx.ws_messages,
            rabbitmq_publish: ctx.rabbitmq_publish,
            redis_conn: ctx.redis_conn,
            invalidate_data: ctx.invalidate_data,
            m: PhantomData::<fn() -> M>,
        };

        match dyn_event {
            DynBroadcastEvent::Joined(module_data, participants) => {
                let mut frontend_data = None;
                let mut participants_data = participants.iter().map(|p| (p.id, None)).collect();

                self.module
                    .on_event(
                        ctx,
                        Event::Joined {
                            frontend_data: &mut frontend_data,
                            participants: &mut participants_data,
                        },
                    )
                    .await?;

                if let Some(frontend_data) = frontend_data {
                    module_data.insert(
                        M::NAMESPACE,
                        serde_json::to_value(frontend_data)
                            .context("Failed to convert frontend-data to value")?,
                    );
                }

                for participant in participants.iter_mut() {
                    if let Some(data) = participants_data.remove(&participant.id).flatten() {
                        let value = serde_json::to_value(data)
                            .context("Failed to convert module peer frontend data to value")?;

                        participant.module_data.insert(M::NAMESPACE, value);
                    }
                }
            }
            DynBroadcastEvent::Leaving => {
                self.module.on_event(ctx, Event::Leaving).await?;
            }
            DynBroadcastEvent::RaiseHand => {
                self.module.on_event(ctx, Event::RaiseHand).await?;
            }
            DynBroadcastEvent::LowerHand => {
                self.module.on_event(ctx, Event::LowerHand).await?;
            }
            DynBroadcastEvent::ParticipantJoined(participant) => {
                let mut data = None;

                self.module
                    .on_event(ctx, Event::ParticipantJoined(participant.id, &mut data))
                    .await?;

                if let Some(data) = data {
                    let value = serde_json::to_value(data)
                        .context("Failed to convert module peer frontend data to value")?;

                    participant.module_data.insert(M::NAMESPACE, value);
                }
            }
            DynBroadcastEvent::ParticipantLeft(participant) => {
                self.module
                    .on_event(ctx, Event::ParticipantLeft(*participant))
                    .await?;
            }
            DynBroadcastEvent::ParticipantUpdated(participant) => {
                let mut data = None;

                self.module
                    .on_event(ctx, Event::ParticipantUpdated(participant.id, &mut data))
                    .await?;

                if let Some(data) = data {
                    let value = serde_json::to_value(data)
                        .context("Failed to convert module peer frontend data to value")?;

                    participant.module_data.insert(M::NAMESPACE, value);
                }
            }
        }

        Ok(())
    }

    async fn destroy(self: Box<Self>, ctx: DestroyContext<'_>) {
        self.module.on_destroy(ctx).await
    }
}

#[async_trait::async_trait(?Send)]
pub trait ModuleBuilder: Send + Sync {
    async fn build(&self, builder: &mut Builder) -> Result<()>;

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
    async fn build(&self, builder: &mut Builder) -> Result<()> {
        let ctx = InitContext {
            id: builder.id,
            room: &builder.room,
            user: &builder.user,
            role: builder.role,
            db: &builder.db,
            rabbitmq_exchanges: &mut builder.rabbitmq_exchanges,
            rabbitmq_bindings: &mut builder.rabbitmq_bindings,
            events: &mut builder.events,
            redis_conn: &mut builder.redis_conn,
            m: PhantomData::<fn() -> M>,
        };

        let module = M::init(ctx, &self.params, builder.protocol).await?;

        builder.modules.add_module(module).await;

        Ok(())
    }

    fn clone_boxed(&self) -> Box<dyn ModuleBuilder> {
        Box::new(Self {
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
