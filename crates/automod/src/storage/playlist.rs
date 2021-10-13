//! Ordered list of participants used by the `Playlist` selection strategy.
//!
//! Depending on the selection strategy:
//!
//! - `none`, `random` or `nomination`: The playlist does not get used by these strategies.
//!
//! - `playlist` The playlist is a ordered list of participants which will get used to select
//!     the next participant when yielding. It is also used as a pool to select participants
//!     randomly from (moderator command `Select`).

use anyhow::{Context, Result};
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:playlist
#[ignore_extra_doc_attributes]
/// Typed key to the automod playlist
struct RoomAutoModPlaylist {
    room: SignalingRoomId,
}

impl_to_redis_args!(RoomAutoModPlaylist);

/// Set the playlist. If the `playlist` parameter is empty the old one will still be removed.
#[tracing::instrument(name = "set_playlist", level = "debug", skip(redis_conn, playlist))]
pub async fn set(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    playlist: &[ParticipantId],
) -> Result<()> {
    redis_conn
        .del(RoomAutoModPlaylist { room })
        .await
        .context("Failed to delete playlist to later reinsert it")?;

    if !playlist.is_empty() {
        redis_conn
            .rpush(RoomAutoModPlaylist { room }, playlist)
            .await
            .context("Failed to insert new list")
    } else {
        Ok(())
    }
}

/// Get the next participant from the playlist
#[tracing::instrument(name = "pop_playlist", level = "debug", skip(redis_conn))]
pub async fn pop(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Option<ParticipantId>> {
    redis_conn
        .lpop(RoomAutoModPlaylist { room }, None)
        .await
        .context("Failed to pop playlist")
}

/// Returns the playlist in a Vec.
#[tracing::instrument(name = "get_playlist", level = "debug", skip(redis_conn))]
pub async fn get_all(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .lrange(RoomAutoModPlaylist { room }, 0, -1)
        .await
        .context("Failed to get_all playlist")
}

/// Remove first occurrence of `participant` from the playlist.
#[tracing::instrument(name = "remove_from_playlist", level = "debug", skip(redis_conn))]
pub async fn remove_first(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .lrem(RoomAutoModPlaylist { room }, 1, participant)
        .await
        .context("Failed to remove participant from playlist")
}

/// Remove all occurrences of `participant` from the playlist.
#[tracing::instrument(
    name = "remove_all_occurences_from_playlist",
    level = "debug",
    skip(redis_conn)
)]
pub async fn remove_all_occurrences(
    redis_conn: &mut ConnectionManager,
    room: SignalingRoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .lrem(RoomAutoModPlaylist { room }, 0, participant)
        .await
        .context("Failed to remove all occurences of participant from playlist")
}

/// Delete the complete playlist.
#[tracing::instrument(name = "del_playlist", level = "debug", skip(redis_conn))]
pub async fn del(redis_conn: &mut ConnectionManager, room: SignalingRoomId) -> Result<()> {
    redis_conn
        .del(RoomAutoModPlaylist { room })
        .await
        .context("Failed to del playlist")
}
