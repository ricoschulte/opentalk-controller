use chrono::{DateTime, Utc};
use controller::prelude::*;
use k3k_ee_chat::{incoming, Chat};
use pretty_assertions::assert_eq;
use serde_json::json;
use serial_test::serial;
use test_util::*;

#[actix_rt::test]
#[serial]
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

    let waiting_room = false;
    let room = test_ctx
        .db_ctx
        .create_test_room(ROOM_ID, user1.id, waiting_room)
        .unwrap();

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
            let json = serde_json::to_value(ee_chat_data).unwrap();
            assert_eq!(
                json,
                json!({
                    "group_messages": [
                        {
                            "history":[],
                            "name":"group1"
                        },
                        {
                            "history":[],
                            "name":"group2"
                        }
                    ],
                    "last_seen_timestamps_group": {}
                })
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
            let json = serde_json::to_value(peer_frontend_data).unwrap();
            assert_eq!(json, json!({"groups": ["group1"]}));

            // check own groups
            let ee_chat_data = module_data.get("ee_chat").unwrap();
            let json = serde_json::to_value(ee_chat_data).unwrap();
            assert_eq!(
                json,
                json!({
                    "group_messages": [
                        {
                            "history": [],
                            "name":"group1"
                        },
                        {
                            "history": [],
                            "name": "group3"
                        }
                    ],
                    "last_seen_timestamps_group": {}
                })
            );
        }
        _ => panic!(),
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn last_seen_timestamps() {
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

    let waiting_room = false;
    let room = test_ctx
        .db_ctx
        .create_test_room(ROOM_ID, user1.id, waiting_room)
        .unwrap();

    let mut module_tester = ModuleTester::<Chat>::new(
        test_ctx.db_ctx.db.clone(),
        test_ctx.authz,
        test_ctx.redis_conn,
        room,
    );

    {
        // join the first user
        module_tester
            .join_user(
                USER_1.participant_id,
                user1.clone(),
                Role::User,
                USER_1.name,
                (),
            )
            .await
            .unwrap();
        // discard the received ws join success message, it is tested elsewhere
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap();
    }

    {
        // join another user in order to keep the room alive when the first
        // user leaves and joins the room
        module_tester
            .join_user(USER_2.participant_id, user2, Role::User, USER_2.name, ())
            .await
            .unwrap();
        // discard the received ws join success message, it is tested elsewhere
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap();
    }

    let timestamp_raw = "2022-01-01T10:11:12Z";
    let timestamp: Timestamp =
        DateTime::<Utc>::from(DateTime::parse_from_rfc3339(timestamp_raw).unwrap()).into();
    let message = incoming::Message::SetLastSeenTimestamp {
        group: "group1".to_string(),
        timestamp,
    };
    module_tester
        .send_ws_message(&USER_1.participant_id, message)
        .unwrap();

    // leave and join again with the first user
    module_tester.leave(&USER_1.participant_id).await.unwrap();
    module_tester
        .join_user(USER_1.participant_id, user1, Role::User, USER_1.name, ())
        .await
        .unwrap();

    let rejoin_success = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    // verify that we receive the correct timestamp for group1
    match rejoin_success {
        controller::prelude::WsMessageOutgoing::Control(
            control::outgoing::Message::JoinSuccess(control::outgoing::JoinSuccess {
                module_data,
                ..
            }),
        ) => {
            // check own groups
            let ee_chat_data = module_data.get("ee_chat").unwrap();
            let json = serde_json::to_value(ee_chat_data).unwrap();
            assert_eq!(
                json,
                json!({
                    "group_messages": [
                        {
                            "history":[],
                            "name":"group1"
                        },
                        {
                            "history":[],
                            "name":"group2"
                        }
                    ],
                    "last_seen_timestamps_group": {
                        "group1": timestamp_raw,
                    }
                })
            );
        }
        _ => panic!(),
    }

    module_tester.shutdown().await.unwrap();
}
