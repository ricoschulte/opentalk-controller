use super::Result;
use crate::api::v1::parse_json_response;
use crate::K3KSession;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Room
///
/// Contains all room information. Is only be accessible to the owner and users with
/// appropriate permissions.
#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct Room {
    pub uuid: Uuid,
    pub owner: i64,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}

/// Public room details
///
/// Contains general public information about a room.
#[derive(Debug, Deserialize)]
pub struct RoomDetails {
    pub uuid: Uuid,
    pub owner: i64,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
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
    pub async fn new_room(&self, new_room: &NewRoom) -> Result<Room> {
        let response = self.post_json_authenticated("/v1/rooms", new_room).await?;

        parse_json_response(response).await
    }

    /// Calls *PUT '/v1/rooms/{uuid}*
    pub async fn modify_room(&self, room_uuid: &Uuid, modify_room: &ModifyRoom) -> Result<Room> {
        let response = self
            .put_json_authenticated(&format!("/v1/rooms/{}", room_uuid), modify_room)
            .await?;

        parse_json_response(response).await
    }

    /// Calls *GET '/v1/rooms/{uuid}*
    pub async fn get_room_by_uuid(&self, room_uuid: &Uuid) -> Result<RoomDetails> {
        let response = self
            .get_authenticated(&format!("/v1/rooms/{}", room_uuid))
            .await?;

        parse_json_response(response).await
    }

    /// Unimplemented
    pub async fn start_room(&self, _room_uuid: Uuid) {
        unimplemented!()
    }
}
