use controller::prelude::serde_json::Value;
use controller::prelude::WsMessageOutgoing;
use k3k_legal_vote::incoming::{Stop, UserParameters, VoteMessage};
use k3k_legal_vote::outgoing::{Response, VoteResponse, VoteResults, Votes};
use k3k_legal_vote::{incoming, outgoing, VoteOption};
use serial_test::serial;
use std::collections::HashMap;
use test_util::*;

mod common;

#[actix_rt::test]
#[serial]
async fn basic_vote() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users(&test_ctx).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        secret: false,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user1
    let vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);

        parameters.vote_id
    } else {
        panic!("Expected Start message")
    };

    // Expect Start response in websocket for user2
    if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.vote_id, vote_id);
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `vote_id` to exist in database
    let legal_vote = test_ctx
        .db_ctx
        .db_conn
        .get_legal_vote(vote_id)
        .unwrap()
        .unwrap();

    assert_eq!(legal_vote.id, vote_id);
    assert_eq!(legal_vote.initiator, USER_1.user_id);
    assert_eq!(legal_vote.protocol, Value::Array(vec![]));

    // Start casting votes

    let vote_success = WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
        vote_id,
        response: Response::Success,
    }));

    // Vote 'Yes' with user1
    let vote_yes = incoming::Message::Vote(VoteMessage {
        vote_id,
        option: VoteOption::Yes,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, vote_yes)
        .unwrap();

    //Expect VoteSuccess
    let vote_response = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(vote_success, vote_response);

    // Expect a vote Update message on all participants
    let mut voters = HashMap::new();
    voters.insert(USER_1.participant_id, VoteOption::Yes);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        votes: Votes {
            yes: 1,
            no: 0,
            abstain: None,
        },
        voters: Some(voters.clone()),
    }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    // Vote 'No' with user2
    let vote_no = incoming::Message::Vote(VoteMessage {
        vote_id,
        option: VoteOption::No,
    });

    module_tester
        .send_ws_message(&USER_2.participant_id, vote_no)
        .unwrap();

    //Expect VoteSuccess
    let vote_response = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(vote_success, vote_response);

    // Expect a vote Update message on all participants
    voters.insert(USER_2.participant_id, VoteOption::No);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        votes: Votes {
            yes: 1,
            no: 1,
            abstain: None,
        },
        voters: Some(voters.clone()),
    }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    // stop vote
    let stop_vote = incoming::Message::Stop(Stop { vote_id });

    module_tester
        .send_ws_message(&USER_1.participant_id, stop_vote)
        .unwrap();

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(VoteResults {
            vote_id,
            votes: Votes {
                yes: 1,
                no: 1,
                abstain: None,
            },
            voters: Some(voters),
        }));

    // expect stop messages for all users
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");

        assert_eq!(expected_stop_message, stop_message);
    }

    // check the vote protocol
    let legal_vote = test_ctx
        .db_ctx
        .db_conn
        .get_legal_vote(vote_id)
        .unwrap()
        .unwrap();

    assert_eq!(legal_vote.id, vote_id);
    assert_eq!(legal_vote.initiator, USER_1.user_id);
    if let Value::Array(protocol) = legal_vote.protocol {
        assert_eq!(protocol.len(), 4);
    }

    module_tester.shutdown().await.unwrap();
}
