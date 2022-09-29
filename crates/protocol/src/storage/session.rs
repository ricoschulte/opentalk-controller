use crate::SessionInfo;
use anyhow::{Context, Result};
use controller::prelude::*;
use controller_shared::ParticipantId;
use displaydoc::Display;
use redis::AsyncCommands;

#[derive(Display)]
/// k3k-signaling:room={room_id}:participant={participant_id}:protocol-session
#[ignore_extra_doc_attributes]
///
/// Contains the [`SessionInfo`] of the a participant.
pub(super) struct SessionInfoKey {
    pub(super) room_id: SignalingRoomId,
    pub(super) participant_id: ParticipantId,
}

impl_to_redis_args!(SessionInfoKey);

#[tracing::instrument(name = "set_protocol_session_info", skip(redis_conn))]
pub(crate) async fn set(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
    session_info: &SessionInfo,
) -> Result<()> {
    redis_conn
        .set(
            SessionInfoKey {
                room_id,
                participant_id,
            },
            session_info,
        )
        .await
        .context("Failed to set protocol session info key")
}

#[tracing::instrument(name = "get_protocol_session_info", skip(redis_conn))]
pub(crate) async fn get(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
) -> Result<Option<SessionInfo>> {
    redis_conn
        .get(SessionInfoKey {
            room_id,
            participant_id,
        })
        .await
        .context("Failed to get protocol session info key")
}

#[tracing::instrument(name = "get_del_protocol_session_info", skip(redis_conn))]
pub(crate) async fn get_del(
    redis_conn: &mut RedisConnection,
    room_id: SignalingRoomId,
    participant_id: ParticipantId,
) -> Result<Option<SessionInfo>> {
    redis::cmd("GETDEL")
        .arg(SessionInfoKey {
            room_id,
            participant_id,
        })
        .query_async(redis_conn)
        .await
        .context("Failed to get_del protocol session info key")
}
