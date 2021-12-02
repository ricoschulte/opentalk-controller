use super::rooms::{RoomDetails, RoomId};
use super::users::UserDetails;
use super::{parse_json_response, Result};
use crate::K3KSession;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct NewInvite {
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct UpdateInvite {
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Invite {
    pub invite_code: String,
    pub created: chrono::DateTime<chrono::Utc>,
    pub created_by: UserDetails,
    pub updated: chrono::DateTime<chrono::Utc>,
    pub updated_by: UserDetails,
    pub room_id: RoomId,
    pub active: bool,
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Clone)]
pub struct InviteCodeId(String);

impl From<Invite> for InviteCodeId {
    fn from(val: Invite) -> Self {
        InviteCodeId(val.invite_code)
    }
}
impl std::fmt::Display for InviteCodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[async_trait::async_trait]
pub trait InviteApi {
    async fn update<T: Into<chrono::DateTime<chrono::Utc>> + Send>(
        &self,
        session: &K3KSession,
        expiration: Option<T>,
    ) -> Result<Invite>;

    async fn delete(self, session: &K3KSession) -> Result<Invite>;
}

#[async_trait::async_trait]
pub trait InviteRoomApiEx: AsRef<RoomId> {
    async fn create_invite(
        &self,
        session: &K3KSession,
        expiration: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Invite>;

    async fn invites(&self, session: &K3KSession) -> Result<Vec<Invite>>;
}

#[async_trait::async_trait]
impl InviteRoomApiEx for RoomDetails {
    async fn create_invite(
        &self,
        session: &K3KSession,
        expiration: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Invite> {
        session.new_invite(self, NewInvite { expiration }).await
    }

    async fn invites(&self, session: &K3KSession) -> Result<Vec<Invite>> {
        session.get_room_invites(self).await
    }
}

#[async_trait::async_trait]
impl InviteApi for Invite {
    async fn update<T: Into<chrono::DateTime<chrono::Utc>> + Send>(
        &self,
        session: &K3KSession,
        expiration: Option<T>,
    ) -> Result<Invite> {
        let id = InviteCodeId(self.invite_code.clone());
        let expiration = expiration.map(Into::into);
        session
            .update_invite(&self.room_id, &id, &UpdateInvite { expiration })
            .await
    }

    async fn delete(self, session: &K3KSession) -> Result<Invite> {
        let room = self.room_id.clone();
        session.delete_invite(room, self).await
    }
}

impl K3KSession {
    pub async fn new_invite<R: AsRef<RoomId>>(
        &self,
        room: R,
        new_invite: NewInvite,
    ) -> Result<Invite> {
        let response = self
            .post_json_authenticated(&format!("/v1/rooms/{}/invites", room.as_ref()), &new_invite)
            .await?;

        parse_json_response(response).await
    }

    pub async fn update_invite<R: AsRef<RoomId>>(
        &self,
        room: R,
        invite: &InviteCodeId,
        update_invite: &UpdateInvite,
    ) -> Result<Invite> {
        let response = self
            .put_json_authenticated(
                &format!("/v1/rooms/{}/invites/{}", room.as_ref(), invite),
                &update_invite,
            )
            .await?;

        parse_json_response(response).await
    }

    pub async fn delete_invite<R: AsRef<RoomId>, I: Into<InviteCodeId>>(
        &self,
        room: R,
        invite: I,
    ) -> Result<Invite> {
        let id = invite.into();
        let response = self
            .delete_authenticated(&format!("/v1/rooms/{}/invites/{}", room.as_ref(), id))
            .await?;

        parse_json_response(response).await
    }

    /// Calls *GET '/v1/rooms/{}/invites*
    pub async fn get_room_invites<R: AsRef<RoomId>>(&self, room: R) -> Result<Vec<Invite>> {
        let response = self
            .get_authenticated(&format!("/v1/rooms/{}/invites", room.as_ref()))
            .await?;

        parse_json_response(response).await
    }
}
