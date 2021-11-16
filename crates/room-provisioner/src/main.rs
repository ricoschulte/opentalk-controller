use anyhow::Result;
use controller::db::migrations::migrate_from_url;
use controller::db::rooms::{NewRoom, RoomId};
use controller::db::users::UserId;
use controller::db::DbInterface;
use controller::settings::Database;
use credentials::Credentials;
use provision::Provision;
use provision::{Room, User};
use uuid::Uuid;

mod credentials;
mod provision;

#[tokio::main]
async fn main() -> Result<()> {
    let path_to_yaml = path_to_yaml();
    let yaml = tokio::fs::read_to_string(path_to_yaml).await?;
    let provision = Provision::from_yaml(&yaml)?;
    let credentials = Credentials::from_environment()?;
    apply_provisioning(provision, credentials).await
}

fn path_to_yaml() -> String {
    std::env::var("PROVISIONING_YAML_PATH").unwrap_or_else(|_| "./provisioning".into())
}

async fn apply_provisioning(provision: Provision, credentials: Credentials) -> Result<()> {
    migrate_from_url(&credentials.postgresql.url).await?;
    create_users(provision.users, &credentials.postgresql)?;
    create_rooms(provision.rooms, &credentials.postgresql)
}

fn create_users(users: Vec<User>, database: &Database) -> Result<()> {
    let database_interface = DbInterface::connect(database)?;
    users
        .into_iter()
        .try_for_each(|user| create_user(user, &database_interface))
}

fn create_user(user: User, database_interface: &DbInterface) -> Result<()> {
    Ok(database_interface.create_user(user.into())?)
}

fn create_rooms(rooms: Vec<Room>, database: &Database) -> Result<()> {
    let database_interface = DbInterface::connect(database)?;
    rooms
        .into_iter()
        .try_for_each(|room| try_to_create_room(room, &database_interface))
}

fn try_to_create_room(room: Room, database_interface: &DbInterface) -> Result<()> {
    if let Some(room_owner) = database_interface.get_user_by_uuid(&room.owner)? {
        return create_room(room, room_owner.id, database_interface);
    }
    println!(
        "Can't create room. User with uuid: {} doesn't exist.",
        room.owner
    );
    Ok(())
}

fn create_room(room: Room, owner_id: UserId, database_interface: &DbInterface) -> Result<()> {
    let new_room = NewRoom {
        uuid: RoomId::from(room.uuid.unwrap_or_else(Uuid::new_v4)),
        owner: owner_id,
        password: room.password,
        wait_for_moderator: room.wait_for_moderator,
        listen_only: room.listen_only,
    };
    database_interface.new_room(new_room)?;
    Ok(())
}
