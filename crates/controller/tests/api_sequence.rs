use anyhow::Result;
use k3k_controller_client::api;
use serial_test::serial;

mod common;

/// Test basic API functionality
///
/// This test needs a keycloak test user in order to succeed. The user should have the following
/// properties:
///
/// ```
/// username: test
/// password: test
/// mail: test@mail.de
/// firstname: test
/// lastname: tester
/// ```
///
/// Calls all exposed API endpoints in their intended manner. None of the requests should fail.
#[tokio::test]
#[serial]
#[ignore]
async fn basic_sequence() -> Result<()> {
    common::setup_logging()?;

    // database will clean up when this gets dropped
    let _db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut controller = common::run_controller().await?;
    let session = common::setup_client("test", "test").await?;

    // Log in
    log::info!("logging in...");
    let permissions = session.login().await?;
    log::info!("login successful, permissions: {:#?}", permissions);

    // Test user profile
    log::info!("get current user profile...");
    let current_user = session.get_current_user().await?;
    log::info!("current user profile: {:#?}", current_user);
    assert_eq!(current_user.email, "test@heinlein-video.de");
    assert_eq!(current_user.firstname, "Test");
    assert_eq!(current_user.lastname, "Tester");
    assert!(current_user.title.is_empty());

    // Test user details
    log::info!("get current user details by id...");
    let user_details = session.get_user_details(current_user.id).await?;
    log::info!("current user details: {:#?}", user_details);
    assert_eq!(user_details.id, current_user.id);
    assert_eq!(user_details.email, current_user.email);
    assert_eq!(user_details.firstname, current_user.firstname);
    assert_eq!(user_details.lastname, current_user.lastname);

    // Test get all users
    log::info!("get all users...");
    let users = session.all_users().await?;
    log::info!("all users: {:#?}", users);
    assert_eq!(users.len(), 1);
    assert!(users.contains(&user_details));

    // Test modify current user
    log::info!("update user to have doctor title...");
    let modify_user = api::v1::users::ModifyUser {
        title: Some("Dr.".to_string()),
        theme: Some("".to_string()),
        language: None,
    };

    let updated_user = session.modify_current_user(&modify_user).await?;
    log::info!("updated user: {:#?}", updated_user);
    assert_eq!(updated_user.id, current_user.id);
    assert_eq!(updated_user.title, "Dr.");
    assert!(updated_user.theme.is_empty());
    assert!(!updated_user.language.is_empty()); // language should not be empty by default and modify should not change it

    // Test new room
    log::info!("create a new room...");
    let new_room = api::v1::rooms::NewRoom {
        password: "password123".to_string(),
        wait_for_moderator: false,
        listen_only: false,
    };

    let room = session.new_room(&new_room).await?;
    log::info!("created room: {:#?}", room);
    assert_eq!(room.owner, current_user.id);
    assert_eq!(room.password, "password123");
    assert_eq!(room.wait_for_moderator, false);
    assert_eq!(room.listen_only, false);

    // Test room details
    log::info!("get room by uuid...");
    let room_details = session.get_room_by_uuid(&room.uuid).await?;
    log::info!("created room details: {:#?}", room_details);
    assert_eq!(room_details.uuid, room.uuid);
    assert_eq!(room_details.owner, current_user.id);
    assert_eq!(room_details.wait_for_moderator, false);
    assert_eq!(room_details.listen_only, false);

    // Test modify room
    log::info!("modifying room...");
    let modify_room = api::v1::rooms::ModifyRoom {
        password: Some("admin123".to_string()),
        wait_for_moderator: Some(true),
        listen_only: None,
    };

    let modified_room = session.modify_room(&room.uuid, &modify_room).await?;
    log::info!("modified room: {:#?}", modified_room);
    assert_eq!(modified_room.uuid, room.uuid);
    assert_eq!(modified_room.owner, current_user.id);
    assert_eq!(modified_room.password, "admin123");
    assert_eq!(modified_room.wait_for_moderator, true);
    assert_eq!(modified_room.listen_only, false);

    // Test owned rooms
    log::info!("get owned rooms...");
    let owned_rooms = session.get_owned_rooms().await?;
    log::info!("owned rooms: {:#?}", owned_rooms);
    assert_eq!(owned_rooms.len(), 1);
    assert!(owned_rooms.contains(&modified_room));

    controller.kill().await?;

    Ok(())
}
