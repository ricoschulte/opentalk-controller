use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize, PartialEq, Debug)]
pub struct Room {
    pub uuid: Option<Uuid>,
    pub owner: Uuid,
    pub password: String,
    pub wait_for_moderator: bool,
    pub listen_only: bool,
}
