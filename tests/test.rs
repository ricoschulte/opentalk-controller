use anyhow::Result;
use k3k_client::k3k::api::v1 as api_v1;
use serial_test::serial;

mod common;

#[tokio::test]
#[serial]
async fn test_basic_functionality() -> Result<()> {
    common::setup_logging()?;
    common::cleanup_database().await?;
    let mut controller = common::run_controller().await?;
    let session = common::setup_client("test", "test").await?;

    let permissions = session.login().await?;
    log::debug!("login successful, permissions: {:#?}", permissions);

    let current_user = session.get_current_user().await?;
    log::debug!("current user profile: {:#?}", current_user);

    let user_details = session.get_user_details(current_user.id).await?;
    log::debug!("current user details: {:#?}", user_details);

    let modify_user = api_v1::users::ModifyUser {
        title: Some("Dr.".to_string()),
        theme: None,
        language: None,
    };

    let updated_user = session.modify_current_user(&modify_user).await?;
    log::debug!("add doctor title: {:#?}", updated_user);

    let modify_user = api_v1::users::ModifyUser {
        title: Some("".to_string()),
        theme: None,
        language: None,
    };

    let updated_user = session.modify_current_user(&modify_user).await?;
    log::debug!("remove doctor title: {:#?}", updated_user);

    let new_room = api_v1::rooms::NewRoom {
        password: "password123".to_string(),
        wait_for_moderator: false,
        listen_only: false,
    };

    let room = session.new_room(&new_room).await?;
    log::debug!("created room: {:#?}", room);

    let room_details = session.get_room_by_uuid(&room.uuid).await?;
    log::debug!("created room details: {:#?}", room_details);

    let modify_room = api_v1::rooms::ModifyRoom {
        password: Some("admin123".to_string()),
        wait_for_moderator: Some(true),
        listen_only: None,
    };

    let modified_room = session.modify_room(&room.uuid, &modify_room).await?;
    log::debug!("modified room: {:#?}", modified_room);

    let owned_rooms = session.get_owned_rooms().await?;
    log::debug!("owned rooms: {:#?}", owned_rooms);

    controller.kill().await?;

    common::cleanup_database().await?;

    Ok(())
}
