use super::*;
use control::outgoing::Message;
use db_storage::users::User;

/// Creates a new [`ModuleTester`] with two users
pub async fn setup_users<M: SignalingModule>(
    test_ctx: &TestContext,
    params: M::Params,
) -> (ModuleTester<M>, User, User) {
    let user1 = test_ctx.db_ctx.create_test_user(USER_1.n, vec![]).unwrap();
    let user2 = test_ctx.db_ctx.create_test_user(USER_2.n, vec![]).unwrap();

    let room = test_ctx.db_ctx.create_test_room(ROOM_ID, user1.id).unwrap();

    let mut module_tester = ModuleTester::new(
        test_ctx.db_ctx.db.clone(),
        test_ctx.authz.clone(),
        test_ctx.redis_conn.clone(),
        room,
    );

    // Join with user1
    module_tester
        .join_user(
            USER_1.participant_id,
            user1.clone(),
            Role::Moderator,
            USER_1.name,
            params.clone(),
        )
        .await
        .unwrap();

    // Expect a JoinSuccess response
    if let WsMessageOutgoing::Control(Message::JoinSuccess(join_success)) = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(join_success.id, USER_1.participant_id);
        assert_eq!(join_success.role, Role::Moderator);
        assert!(join_success.participants.is_empty());
    } else {
        panic!("Expected ParticipantJoined Event ")
    }

    // Join with user2
    module_tester
        .join_user(
            USER_2.participant_id,
            user2.clone(),
            Role::User,
            USER_2.name,
            params.clone(),
        )
        .await
        .unwrap();

    // Expect a JoinSuccess on user2 websocket
    if let WsMessageOutgoing::Control(Message::JoinSuccess(join_success)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(join_success.id, USER_2.participant_id);
        assert_eq!(join_success.role, Role::User);
        assert_eq!(join_success.participants.len(), 1);
    } else {
        panic!("Expected JoinSuccess message");
    }

    // Expect a ParticipantJoined event on user1 websocket
    if let WsMessageOutgoing::Control(Message::Joined(participant)) = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(participant.id, USER_2.participant_id);
    } else {
        panic!("Expected ParticipantJoined Event ")
    }

    (module_tester, user1, user2)
}
