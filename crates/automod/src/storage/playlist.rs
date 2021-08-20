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
use controller::db::rooms::RoomId;
use controller::prelude::*;
use displaydoc::Display;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:playlist
#[ignore_extra_doc_attributes]
/// Typed key to the automod playlist
struct RoomAutoModPlaylist {
    room: RoomId,
}

impl_to_redis_args!(RoomAutoModPlaylist);

/// Set the playlist. If the `playlist` parameter is empty the old one will still be removed.
pub async fn set(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
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
pub async fn pop(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<Option<ParticipantId>> {
    redis_conn
        .lpop(RoomAutoModPlaylist { room }, None)
        .await
        .context("Failed to pop playlist")
}

/// Returns the playlist in a Vec.
pub async fn get_all(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
) -> Result<Vec<ParticipantId>> {
    redis_conn
        .lrange(RoomAutoModPlaylist { room }, 0, -1)
        .await
        .context("Failed to get_all playlist")
}

/// Remove first occurrence of `participant` from the playlist.
pub async fn remove_first(
    redis_conn: &mut ConnectionManager,
    room: RoomId,
    participant: ParticipantId,
) -> Result<()> {
    redis_conn
        .lrem(RoomAutoModPlaylist { room }, 1, participant)
        .await
        .context("Failed to remove participant from playlist")
}

/// Delete the complete playlist.
pub async fn del(redis_conn: &mut ConnectionManager, room: RoomId) -> Result<()> {
    redis_conn
        .del(RoomAutoModPlaylist { room })
        .await
        .context("Failed to del playlist")
}
