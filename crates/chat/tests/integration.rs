use chrono::{DateTime, Utc};
use controller::prelude::*;
use k3k_chat::{incoming, Chat, Scope};
use serde_json::json;
use serial_test::serial;
use test_util::{TestContext, ROOM_ID, USER_1, USER_2};

#[actix_rt::test]
#[serial]
async fn last_seen_timestamps() {
    let test_ctx = TestContext::new().await;

    let user1 = test_ctx.db_ctx.create_test_user(USER_1.n, vec![]).unwrap();
    let user2 = test_ctx.db_ctx.create_test_user(USER_2.n, vec![]).unwrap();

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
        let join_success = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap();
        match join_success {
            controller::prelude::WsMessageOutgoing::Control(
                control::outgoing::Message::JoinSuccess(control::outgoing::JoinSuccess {
                    module_data,
                    ..
                }),
            ) => {
                // check that last seen timestamps are not set
                let chat_data = module_data.get("chat").unwrap();
                let json = serde_json::to_value(chat_data).unwrap();
                assert_eq!(
                    json,
                    json!({
                        "enabled": true,
                        "last_seen_timestamp_global": null,
                        "last_seen_timestamps_private": {},
                        "room_history": [],
                    })
                );
            }
            _ => panic!(),
        }
    }

    {
        // join another user in order to keep the room alive when the first
        // user leaves and joins the room
        module_tester
            .join_user(USER_2.participant_id, user2, Role::User, USER_2.name, ())
            .await
            .unwrap();
        // discard the received ws join success message, no need to test it here
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap();
    }

    let timestamp_global_raw = "2022-01-01T10:11:12Z";
    let timestamp_private_raw = "2023-04-05T06:07:08Z";

    {
        // set global timestamp
        let timestamp: Timestamp =
            DateTime::<Utc>::from(DateTime::parse_from_rfc3339(timestamp_global_raw).unwrap())
                .into();
        let message = incoming::Message::SetLastSeenTimestamp {
            scope: Scope::Global,
            timestamp,
        };
        module_tester
            .send_ws_message(&USER_1.participant_id, message)
            .unwrap();
    }

    {
        // set private timestamp for chat with user2
        let timestamp: Timestamp =
            DateTime::<Utc>::from(DateTime::parse_from_rfc3339(timestamp_private_raw).unwrap())
                .into();
        let message = incoming::Message::SetLastSeenTimestamp {
            scope: Scope::Private(USER_2.participant_id),
            timestamp,
        };
        module_tester
            .send_ws_message(&USER_1.participant_id, message)
            .unwrap();
    }

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
            let chat_data = module_data.get("chat").unwrap();
            let json = serde_json::to_value(chat_data).unwrap();
            assert_eq!(
                json,
                json!({
                    "enabled": true,
                    "last_seen_timestamp_global": timestamp_global_raw,
                    "last_seen_timestamps_private": {
                        "00000000-0000-0000-0000-000000000002": timestamp_private_raw,
                    },
                    "room_history": [],
                })
            );
        }
        _ => panic!(),
    }

    module_tester.shutdown().await.unwrap();
}
