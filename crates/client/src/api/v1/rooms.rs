use super::Result;
use crate::api::v1::parse_json_response;
use crate::K3KSession;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Deref};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
pub struct RoomId(Uuid);
impl Deref for RoomId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<RoomId> for RoomId {
    fn as_ref(&self) -> &RoomId {
        self
    }
}

impl Display for RoomId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Room> for RoomId {
    fn from(val: Room) -> Self {
        val.uuid
    }
}

impl From<RoomDetails> for RoomId {
    fn from(val: RoomDetails) -> Self {
        val.uuid
    }
}

/// A Room
///
/// Contains all room information. Is only be accessible to the owner and users with
/// appropriate permissions.
#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct Room {
    pub uuid: RoomId,
    pub owner: i64,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

impl AsRef<RoomId> for Room {
    fn as_ref(&self) -> &RoomId {
        &self.uuid
    }
}

/// Public room details
///
/// Contains general public information about a room.
#[derive(Debug, Clone, Deserialize)]
pub struct RoomDetails {
    pub uuid: RoomId,
    pub owner: i64,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

impl AsRef<RoomId> for RoomDetails {
    fn as_ref(&self) -> &RoomId {
        &self.uuid
    }
}

/// API request parameters to create a new room
#[derive(Debug, Serialize)]
pub struct NewRoom {
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// API request parameters to modify a room.
#[derive(Debug, Serialize)]
pub struct ModifyRoom {
    pub password: Option<String>,
    pub wait_for_moderator: Option<bool>,
    pub listen_only: Option<bool>,
}

impl K3KSession {
    /// Calls *GET '/v1/rooms*
    pub async fn get_owned_rooms(&self) -> Result<Vec<Room>> {
        let response = self.get_authenticated("/v1/rooms").await?;

        parse_json_response(response).await
    }

    /// Calls *POST '/v1/rooms*
    ///
    /// Consumes the NewRoom, because it basically is a conversion.
    // TODO(r.floren) in the error case, should this give back the ownership of NewRoom?
    pub async fn new_room(&self, new_room: NewRoom) -> Result<Room> {
        let response = self.post_json_authenticated("/v1/rooms", &new_room).await?;

        parse_json_response(response).await
    }

    /// Calls *PUT '/v1/rooms/{uuid}*
    pub async fn modify_room<U: AsRef<RoomId>>(
        &self,
        room_uuid: U,
        modify_room: &ModifyRoom,
    ) -> Result<Room> {
        let response = self
            .put_json_authenticated(&format!("/v1/rooms/{}", (room_uuid.as_ref())), modify_room)
            .await?;

        parse_json_response(response).await
    }

    /// Calls *GET '/v1/rooms/{uuid}*
    pub async fn get_room<U: AsRef<RoomId>>(&self, room_uuid: U) -> Result<RoomDetails> {
        let response = self
            .get_authenticated(&format!("/v1/rooms/{}", room_uuid.as_ref()))
            .await?;

        parse_json_response(response).await
    }

    /// Calls *DELETE '/v1/rooms/{uuid}*
    ///
    /// Consumes the room, to symbolize this resource in no longer valid.
    pub async fn delete_room<U: Into<RoomId>>(&self, room: U) -> Result<()> {
        let uuid = room.into();
        let _response = self
            .delete_authenticated(&format!("/v1/rooms/{}", uuid))
            .await?;
        Ok(())
    }

    /// Unimplemented
    pub async fn start_room(&self, _room_uuid: Uuid) {
        unimplemented!()
    }
}

#[async_trait::async_trait]
pub trait RoomApi {
    async fn create(session: &K3KSession) -> Result<RoomDetails>;
    async fn update(&self, session: &K3KSession) -> Result<RoomDetails>;
    async fn delete(self, session: &K3KSession) -> Result<()>;
}

#[async_trait::async_trait]
impl RoomApi for RoomDetails {
    async fn create(_session: &K3KSession) -> Result<RoomDetails> {
        todo!();
    }
    async fn update(&self, _session: &K3KSession) -> Result<RoomDetails> {
        todo!();
    }
    async fn delete(self, session: &K3KSession) -> Result<()> {
        session.delete_room(self).await
    }
}
