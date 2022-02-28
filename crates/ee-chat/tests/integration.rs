use controller::prelude::*;
use k3k_ee_chat::Chat;
use test_util::*;

#[actix_rt::test]
async fn common_groups_on_join() {
    let test_ctx = TestContext::new().await;

    let user1 = test_ctx
        .db_ctx
        .create_test_user(
            USER_1.n,
            vec![String::from("group1"), String::from("group2")],
        )
        .unwrap();

    let user2 = test_ctx
        .db_ctx
        .create_test_user(
            USER_2.n,
            vec![String::from("group1"), String::from("group3")],
        )
        .unwrap();

    let room = test_ctx.db_ctx.create_test_room(ROOM_ID, user1.id).unwrap();

    let mut module_tester = ModuleTester::<Chat>::new(
        test_ctx.db_ctx.db.clone(),
        test_ctx.authz,
        test_ctx.redis_conn,
        room,
    );

    module_tester
        .join_user(USER_1.participant_id, user1, Role::User, USER_1.name, ())
        .await
        .unwrap();

    let join_success1 = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    match join_success1 {
        controller::prelude::WsMessageOutgoing::Control(
            control::outgoing::Message::JoinSuccess(control::outgoing::JoinSuccess {
                module_data,
                participants,
                ..
            }),
        ) => {
            assert!(participants.is_empty());

            // check own groups
            let ee_chat_data = module_data.get("ee_chat").unwrap();
            let json = serde_json::to_string(&ee_chat_data).unwrap();
            assert_eq!(
                json,
                r#"[{"history":[],"name":"group1"},{"history":[],"name":"group2"}]"#
            );
        }
        _ => panic!(),
    }

    module_tester
        .join_user(USER_2.participant_id, user2, Role::User, USER_2.name, ())
        .await
        .unwrap();

    let join_success2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    match join_success2 {
        controller::prelude::WsMessageOutgoing::Control(
            control::outgoing::Message::JoinSuccess(control::outgoing::JoinSuccess {
                module_data,
                participants,
                ..
            }),
        ) => {
            assert_eq!(participants.len(), 1);

            // check common groups here
            let peer_frontend_data = participants[0].module_data.get("ee_chat").unwrap();
            let json = serde_json::to_string(&peer_frontend_data).unwrap();
            assert_eq!(json, r#"{"groups":["group1"]}"#);

            // check own groups
            let ee_chat_data = module_data.get("ee_chat").unwrap();
            let json = serde_json::to_string(&ee_chat_data).unwrap();
            assert_eq!(
                json,
                r#"[{"history":[],"name":"group1"},{"history":[],"name":"group3"}]"#
            );
        }
        _ => panic!(),
    }

    module_tester.shutdown().await.unwrap();
}
