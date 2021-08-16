//! # Auto-Moderation module
//!
//! ## Functionality
//!
//! On room startup the automod is disabled.
//!
//! Selecting the options for the automod is managed by the frontend and this module does not
//! provide templates or anything else
//!
//! Unlike other modules the automod has commands with different levels of required permissions.
//! These permissions are not yet defined, thus only the room owner is moderator.
//!
//! Following selection_strategies are defined:
//!
//! - `None`: No automatic reselection happens after the current speaker yields. The next one must
//!     always be selected by the moderator. The moderator may choose a participant directly
//!     or let the controller choose one randomly. For that the controller holds a `allow_list`
//!     which is a set of participants which are able to be randomly selected. Furthermore the
//!     controller will hold a list of start/stop speaker events. That list can be used to avoid
//!     double selections (option) when randomly choosing a participant.
//!
//! - `Playlist`: The playlist-strategy requires a playlist of participants. This list will be
//!     stored ordered inside the controller. Whenever a speaker yields the controller will
//!     automatically choose the next participant in the list to be the next speaker.
//!
//!     A moderator may choose to skip over a speaker. That can be done by selecting the next one or
//!     let the controller choose someone random from the playlist.
//!     The playlist can, while the automod is active, be edited.
//!
//! - `Random`: This strategy behaves like `None` but will always choose the next speaker
//!     randomly from the `allow_list` as soon as the current speaker yields.
//!
//! - `Nomination`: This strategy behaves like `None` but requires the current speaker to nominate
//!     the next participant to be speaker. The nominated participant MUST be inside the
//!     `allow_list` and if double selection is not enabled the controller will check if the
//!     nominated participant already was a speaker.
//!
//! ### Lifecycle
//!
//! As soon if a moderator starts the automod, the automod-module of that
//! participant will set the config inside the storage and then send a start message to all other
//! participants.
//!
//! To avoid multiple concurrent actions the module will acquire a redlock to signal the
//! ownership of the automod while doing the work.
//!
//! Receiving the start-message will not change the state of the automod module. Instead it reads
//! out the config from the message and forwards it to the frontend after removing the list of
//! participants if the parameters requires it.
//!
//! The selection of the first speaker must be done by the frontend then, depending of the
//! `selection_strategy`, will the automod continue running until finished or stopped.
//!
//! Once the active speaker yields or it's time runs out, its automod module is responsible to
//! select the next speaker (if the `selection_strategy` requires it). This behaviour MUST only
//! be executed after ensuring that this participant is in fact still the speaker.
//!
//! If the participant leaves while being speaker, its automod-module must execute the same
//! behaviour as if the participants simply yielded without selecting the next one (which would be
//! required for the `nominate` `selection_strategy`. A moderator has to intervene in this
//! situation).
//!
//! Moderators will always be able to execute a re-selection of the current speaker regardless of
//! the `selection_strategy`.

use self::config::{FrontendConfig, PublicConfig, SelectionStrategy, StorageConfig};
use crate::api::signaling::ws::{
    DestroyContext, Event, InitContext, ModuleContext, SignalingModule,
};
use crate::api::signaling::ws_modules::control;
use crate::api::signaling::{ParticipantId, Role};
use anyhow::Context;
use anyhow::Result;
use chrono::Utc;
use rabbitmq::Message;
use serde::Serialize;
use uuid::Uuid;

mod config;
mod incoming;
mod outgoing;
mod rabbitmq;
mod state_machine;
mod storage;

pub struct AutoMod {
    id: ParticipantId,
    room: Uuid,
    role: Role,
}

/// Data sent to the frontend on `join_success`, when automod is active.
#[derive(Debug, Serialize)]
pub struct FrontendData {
    config: PublicConfig,
    speaker: Option<ParticipantId>,
}

#[async_trait::async_trait(?Send)]
impl SignalingModule for AutoMod {
    const NAMESPACE: &'static str = "automod";
    type Params = ();
    type Incoming = incoming::Message;
    type Outgoing = outgoing::Message;
    type RabbitMqMessage = rabbitmq::Message;
    type ExtEvent = ();
    type FrontendData = FrontendData;
    type PeerFrontendData = ();

    async fn init(
        ctx: InitContext<'_, Self>,
        _params: &Self::Params,
        _protocol: &'static str,
    ) -> Result<Self> {
        Ok(Self {
            id: ctx.participant_id(),
            room: ctx.room().uuid,
            role: ctx.role(),
        })
    }

    async fn on_event(
        &mut self,
        ctx: ModuleContext<'_, Self>,
        event: Event<'_, Self>,
    ) -> Result<()> {
        match event {
            Event::Joined {
                frontend_data,
                participants: _,
            } => self.on_joined(ctx, frontend_data).await,
            Event::Leaving => self.on_leaving(ctx).await,
            Event::ParticipantJoined(_, _)
            | Event::ParticipantLeft(_)
            | Event::ParticipantUpdated(_, _) => {
                // ignored
                Ok(())
            }
            Event::WsMessage(msg) => self.on_ws_message(ctx, msg).await,
            Event::RabbitMq(msg) => {
                self.on_rabbitmq_msg(ctx, msg);
                Ok(())
            }
            Event::Ext(_) => unreachable!(),
        }
    }

    async fn on_destroy(self, mut ctx: DestroyContext<'_>) {
        if ctx.destroy_room() {
            let _ = storage::config::del(ctx.redis_conn(), self.room).await;
            let _ = storage::allow_list::del(ctx.redis_conn(), self.room).await;
            let _ = storage::playlist::del(ctx.redis_conn(), self.room).await;
            let _ = storage::history::del(ctx.redis_conn(), self.room).await;
        }
    }
}

/// Convenience macro to unlock a mutex guard and return an error with some context if it fails
macro_rules! try_unlock {
    ($ctx:ident, $guard:ident) => {
        $guard
            .unlock($ctx.redis_conn())
            .await
            .context("Failed to unlock automod redlock")?;
    };
}

/// Macro to try an operation and if it fails unlock the guard and return an error
macro_rules! try_or_unlock {
    ($expr:expr; $ctx:ident, $guard:ident) => {
        match $expr {
            Ok(item) => item,
            Err(e) => {
                try_unlock!($ctx, $guard);
                ::anyhow::bail!(e)
            }
        }
    };
}

impl AutoMod {
    /// Called when participant joins a room.
    ///
    /// Checks if the automod is active by reading the config. If active set the `frontend_data`
    /// to the current config and automod state (history + remaining).
    async fn on_joined(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        frontend_data: &mut Option<FrontendData>,
    ) -> Result<()> {
        let mut mutex = storage::lock::new(self.room);
        let guard = mutex.lock(ctx.redis_conn()).await?;

        let config =
            try_or_unlock!(storage::config::get(ctx.redis_conn(), self.room).await; ctx, guard);

        // If config is some, automod is active and running
        let config = if let Some(config) = config {
            config
        } else {
            // automod not active, just return nothing
            try_unlock!(ctx, guard);
            return Ok(());
        };

        let speaker =
            try_or_unlock!(storage::speaker::get(ctx.redis_conn(), self.room).await; ctx, guard);

        let history = try_or_unlock!(storage::history::get(ctx.redis_conn(), self.room, config.started).await; ctx, guard);

        let remaining = match config.parameter.selection_strategy {
            SelectionStrategy::None | SelectionStrategy::Random | SelectionStrategy::Nomination => {
                vec![]
            }
            SelectionStrategy::Playlist => {
                try_or_unlock!(storage::playlist::get_all(ctx.redis_conn(), self.room).await; ctx, guard)
            }
        };

        *frontend_data = Some(FrontendData {
            config: FrontendConfig {
                parameter: config.parameter,
                history,
                remaining,
            }
            .into_public(),
            speaker,
        });

        try_unlock!(ctx, guard);

        Ok(())
    }

    /// Called right before a participants leaves.
    ///
    /// Removes this participants from the playlist, allow_list and sets the speaker to none if
    /// speaker is the leaving participant.
    async fn on_leaving(&mut self, mut ctx: ModuleContext<'_, Self>) -> Result<()> {
        let mut mutex = storage::lock::new(self.room);
        let guard = mutex.lock(ctx.redis_conn()).await?;

        let config =
            try_or_unlock!(storage::config::get(ctx.redis_conn(), self.room).await; ctx, guard);

        if let Some(config) = config {
            try_or_unlock!(storage::playlist::remove_first(ctx.redis_conn(), self.room, self.id).await; ctx, guard);
            try_or_unlock!(storage::allow_list::remove(ctx.redis_conn(), self.room, self.id).await; ctx, guard);

            let speaker = try_or_unlock!(storage::speaker::get(ctx.redis_conn(), self.room).await; ctx, guard);

            if speaker == Some(self.id) {
                try_or_unlock!(self.select_next(&mut ctx, config, None).await; ctx, guard);
            }
        }

        try_unlock!(ctx, guard);

        Ok(())
    }

    async fn on_ws_message(
        &mut self,
        mut ctx: ModuleContext<'_, Self>,
        msg: incoming::Message,
    ) -> Result<()> {
        match self.role {
            Role::User if msg.requires_moderator_privileges() => {
                ctx.ws_send(outgoing::Message::Error(
                    outgoing::Error::InsufficientPermissions,
                ));

                return Ok(());
            }
            _ => {}
        }

        let mut mutex = storage::lock::new(self.room);

        match msg {
            incoming::Message::Start(incoming::Start {
                parameter,
                allow_list,
                playlist,
            }) => {
                let guard = mutex.lock(ctx.redis_conn()).await?;

                let started = Utc::now();
                let config = StorageConfig { started, parameter };

                let remaining = match config.parameter.selection_strategy {
                    SelectionStrategy::None
                    | SelectionStrategy::Random
                    | SelectionStrategy::Nomination => {
                        try_or_unlock!(storage::allow_list::set(ctx.redis_conn(), self.room, &allow_list).await; ctx,guard);
                        vec![]
                    }
                    SelectionStrategy::Playlist => {
                        try_or_unlock!(storage::playlist::set(ctx.redis_conn(), self.room, &playlist).await; ctx,guard);
                        playlist
                    }
                };

                try_or_unlock!(
                    storage::config::set(ctx.redis_conn(), self.room,  &config).await;
                    ctx,
                    guard
                );

                ctx.rabbitmq_publish(
                    control::rabbitmq::room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::Start(rabbitmq::Start {
                        frontend_config: FrontendConfig {
                            parameter: config.parameter,
                            history: vec![],
                            remaining,
                        },
                    }),
                );

                try_unlock!(ctx, guard);
            }
            incoming::Message::Edit(edit) => {
                let guard = mutex.lock(ctx.redis_conn()).await?;

                let config = try_or_unlock!(storage::config::get(ctx.redis_conn(), self.room).await; ctx, guard);

                // only edit if automod is active
                if let Some(config) = config {
                    // set playlist if requested
                    if let Some(playlist) = &edit.playlist {
                        try_or_unlock!(
                            storage::playlist::set(ctx.redis_conn(), self.room, playlist).await;
                            ctx,
                            guard
                        );
                    }

                    // set allow_list if requested
                    if let Some(allow_list) = &edit.allow_list {
                        try_or_unlock!(
                            storage::allow_list::set(ctx.redis_conn(), self.room, allow_list).await;
                            ctx,
                            guard
                        );
                    }

                    // depending on the strategy find out if the remaining-list has been changed
                    let remaining = match config.parameter.selection_strategy {
                        SelectionStrategy::None
                        | SelectionStrategy::Random
                        | SelectionStrategy::Nomination => edit.allow_list,
                        SelectionStrategy::Playlist => edit.playlist,
                    };

                    // publish remaining update if remaining changed
                    if let Some(remaining) = remaining {
                        ctx.rabbitmq_publish(
                            control::rabbitmq::room_exchange_name(self.room),
                            control::rabbitmq::room_all_routing_key().into(),
                            rabbitmq::Message::RemainingUpdate(rabbitmq::RemainingUpdate {
                                remaining,
                            }),
                        );
                    }
                }

                try_unlock!(ctx, guard);
            }
            incoming::Message::Stop => {
                let guard = mutex.lock(ctx.redis_conn()).await?;

                let config = try_or_unlock!(storage::config::get(ctx.redis_conn(), self.room).await; ctx, guard);

                if config.is_some() {
                    try_or_unlock!(storage::config::del(ctx.redis_conn(), self.room).await; ctx, guard);
                    try_or_unlock!(storage::allow_list::del(ctx.redis_conn(), self.room).await; ctx, guard);
                    try_or_unlock!(storage::playlist::del(ctx.redis_conn(), self.room).await; ctx, guard);

                    ctx.rabbitmq_publish(
                        control::rabbitmq::room_exchange_name(self.room),
                        control::rabbitmq::room_all_routing_key().into(),
                        rabbitmq::Message::Stop,
                    );
                }

                try_unlock!(ctx, guard);
            }
            incoming::Message::Select(select) => {
                let guard = mutex.lock(ctx.redis_conn()).await?;
                let config = try_or_unlock!(storage::config::get(ctx.redis_conn(), self.room).await; ctx, guard);

                if let Some(config) = config {
                    match select {
                        incoming::Select::None => {
                            try_or_unlock!(
                                self.select_specific(&mut ctx, config, None).await;
                                ctx,
                                guard
                            );
                        }
                        incoming::Select::Random => {
                            try_or_unlock!(
                                self.select_random(&mut ctx, config).await;
                                ctx,
                                guard
                            );
                        }
                        incoming::Select::Specific(specific) => {
                            try_or_unlock!(
                                self.select_specific(&mut ctx, config, Some(specific.participant)).await;
                                ctx,
                                guard
                            );
                        }
                    }
                }

                try_unlock!(ctx, guard);
            }
            incoming::Message::Yield(incoming::Yield { next }) => {
                let guard = mutex.lock(ctx.redis_conn()).await?;
                let config = try_or_unlock!(storage::config::get(ctx.redis_conn(), self.room).await; ctx, guard);

                if let Some(config) = config {
                    let speaker = try_or_unlock!(storage::speaker::get(ctx.redis_conn(), self.room).await; ctx, guard);

                    // check if current speaker is self
                    if speaker != Some(self.id) {
                        ctx.ws_send(outgoing::Message::Error(
                            outgoing::Error::InsufficientPermissions,
                        ));

                        try_unlock!(ctx, guard);

                        return Ok(());
                    }

                    try_or_unlock!(self.select_next(&mut ctx, config, next).await; ctx, guard);
                }

                try_unlock!(ctx, guard);
            }
        }

        Ok(())
    }

    /// Selects a specific participant to be the next speaker. Will check the participant if
    /// selection is valid.
    async fn select_specific(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        config: StorageConfig,
        participant: Option<ParticipantId>,
    ) -> Result<()> {
        // when selecting a specific participant check if its a valid one
        if let Some(participant) = participant {
            let is_valid =
                control::storage::participants_contains(ctx.redis_conn(), self.room, participant)
                    .await?;

            if !is_valid {
                return Ok(());
            }
        }

        let result =
            state_machine::select_unchecked(ctx.redis_conn(), self.room, &config, participant)
                .await;

        self.handle_selection_result(ctx, result).await
    }

    /// Selects a random participant to be the next speaker.
    async fn select_random(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        config: StorageConfig,
    ) -> Result<()> {
        let result = state_machine::select_random(
            ctx.redis_conn(),
            self.room,
            &config,
            &mut rand::thread_rng(),
        )
        .await;

        self.handle_selection_result(ctx, result).await
    }

    async fn select_next(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        config: StorageConfig,
        nominated: Option<ParticipantId>,
    ) -> Result<()> {
        let result = state_machine::select_next(
            ctx.redis_conn(),
            self.room,
            &config,
            nominated,
            &mut rand::thread_rng(),
        )
        .await;

        self.handle_selection_result(ctx, result).await
    }

    async fn handle_selection_result(
        &mut self,
        ctx: &mut ModuleContext<'_, Self>,
        result: Result<Option<rabbitmq::SpeakerUpdate>, state_machine::Error>,
    ) -> Result<()> {
        match result {
            Ok(Some(update)) => {
                ctx.rabbitmq_publish(
                    control::rabbitmq::room_exchange_name(self.room),
                    control::rabbitmq::room_all_routing_key().into(),
                    rabbitmq::Message::SpeakerUpdate(update),
                );

                Ok(())
            }
            Ok(None) => Ok(()),
            Err(state_machine::Error::InvalidSelection) => {
                ctx.ws_send(outgoing::Message::Error(outgoing::Error::InvalidSelection));

                Ok(())
            }
            Err(state_machine::Error::Fatal(e)) => Err(e),
        }
    }

    fn on_rabbitmq_msg(&mut self, mut ctx: ModuleContext<'_, Self>, msg: rabbitmq::Message) {
        match msg {
            rabbitmq::Message::Start(rabbitmq::Start { frontend_config }) => {
                ctx.ws_send(outgoing::Message::Started(frontend_config.into_public()));
            }
            rabbitmq::Message::Stop => {
                ctx.ws_send(outgoing::Message::Stopped);
            }
            rabbitmq::Message::SpeakerUpdate(rabbitmq::SpeakerUpdate {
                speaker,
                history,
                remaining,
            }) => ctx.ws_send(outgoing::Message::SpeakerUpdated(
                outgoing::SpeakerUpdated {
                    speaker,
                    history,
                    remaining,
                },
            )),
            Message::RemainingUpdate(rabbitmq::RemainingUpdate { remaining }) => ctx.ws_send(
                outgoing::Message::RemainingUpdated(outgoing::RemainingUpdated { remaining }),
            ),
        }
    }
}
