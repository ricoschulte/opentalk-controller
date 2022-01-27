//! Contains and reexports all functions required to select a speaker from the state machine.
//!
//! The state machine stores its state complete exclusively inside Redis. See the `storage` module
//! for more information.

use crate::config::{SelectionStrategy, StorageConfig};
use crate::rabbitmq;
use crate::storage;
use anyhow::Result;
use controller::prelude::*;
use controller_shared::ParticipantId;
use redis::aio::ConnectionManager;

mod next;
mod random;

pub use next::select_next;
pub use random::select_random;

/// Error returned by the state machine
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The user made an invalid selection, either the participant does not exist or isn't eligible
    /// for selection.
    #[error("invalid selection")]
    InvalidSelection,

    /// A general fatal error has occurred (bug or IO)
    #[error("{:?}", 0)]
    Fatal(#[from] anyhow::Error),
}

#[derive(Debug, PartialEq)]
pub enum StateMachineOutput {
    SpeakerUpdate(rabbitmq::SpeakerUpdate),
    StartAnimation(rabbitmq::StartAnimation),
}

pub fn map_select_unchecked(
    output: Result<Option<rabbitmq::SpeakerUpdate>, Error>,
) -> Result<Option<StateMachineOutput>, Error> {
    output.map(|opt| opt.map(StateMachineOutput::SpeakerUpdate))
}

/// Selects the given participant (or None) as the current speaker and generates the appropriate
/// [`SpeakerUpdate`] if necessary.
/// Does not check if the participant exists or is even eligible to be speaker.
pub async fn select_unchecked(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    config: &StorageConfig,
    participant: Option<ParticipantId>,
) -> Result<Option<rabbitmq::SpeakerUpdate>, Error> {
    let previous = if let Some(participant) = participant {
        storage::speaker::set(redis_conn, room, participant).await?
    } else {
        storage::speaker::del(redis_conn, room).await?
    };

    if previous.is_none() && participant.is_none() {
        // nothing changed, return early
        return Ok(None);
    }

    // If there was a previous speaker add stop event to history
    if let Some(previous) = previous {
        storage::history::add(redis_conn, room, &storage::history::Entry::stop(previous)).await?;
    }

    // If there is a new speaker add start event to history
    if let Some(participant) = participant {
        storage::history::add(
            redis_conn,
            room,
            &storage::history::Entry::start(participant),
        )
        .await?;
    }

    let history = storage::history::get(redis_conn, room, config.started).await?;

    let remaining = match config.parameter.selection_strategy {
        SelectionStrategy::None | SelectionStrategy::Random | SelectionStrategy::Nomination => {
            Some(storage::allow_list::get_all(redis_conn, room).await?)
        }
        SelectionStrategy::Playlist => Some(storage::playlist::get_all(redis_conn, room).await?),
    };

    Ok(Some(rabbitmq::SpeakerUpdate {
        speaker: participant,
        history: Some(history).filter(|history| !history.is_empty()),
        remaining,
    }))
}

#[cfg(test)]
mod test {
    use chrono::{DateTime, Utc};
    use controller::db::rooms::RoomId;
    use controller::prelude::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use redis::aio::ConnectionManager;
    use std::time::{Duration, SystemTime};

    pub const ROOM: SignalingRoomId = SignalingRoomId::new_test(RoomId::from(uuid::Uuid::nil()));

    pub async fn setup() -> ConnectionManager {
        let redis_url =
            std::env::var("REDIS_ADDR").unwrap_or_else(|_| "redis://0.0.0.0:6379/".to_owned());
        let redis = redis::Client::open(redis_url).expect("Invalid redis url");

        let mut mgr = ConnectionManager::new(redis).await.unwrap();

        redis::cmd("FLUSHALL")
            .query_async::<_, ()>(&mut mgr)
            .await
            .unwrap();

        mgr
    }

    pub fn rng() -> StdRng {
        StdRng::seed_from_u64(0)
    }

    pub fn unix_epoch(secs: u64) -> DateTime<Utc> {
        DateTime::from(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }
}
