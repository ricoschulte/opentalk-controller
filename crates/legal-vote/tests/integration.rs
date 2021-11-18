use controller::db::legal_votes::VoteId;
use controller::prelude::serde_json::Value;
use controller::prelude::uuid::Uuid;
use controller::prelude::*;
use controller::prelude::{ModuleTester, ParticipantId, WsMessageOutgoing};
use k3k_legal_vote::incoming::{Cancel, Stop, UserParameters, VoteMessage};
use k3k_legal_vote::outgoing::{
    Canceled, ErrorKind, GuestParticipants, InvalidFields, Response, Stopped, VoteFailed,
    VoteResponse, VoteResults, VoteSuccess, Votes,
};
use k3k_legal_vote::rabbitmq::{self, CancelReason, Parameters};
use k3k_legal_vote::{incoming, outgoing, LegalVote, VoteOption};
use serial_test::serial;
use std::collections::HashMap;
use std::time::Duration;
use test_util::*;

#[actix_rt::test]
#[serial]
async fn basic_vote() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.max_votes, 2);

        parameters.vote_id
    } else {
        panic!("Expected Start message")
    };

    // Expect Start response in websocket for user 2
    if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.vote_id, vote_id);
        assert_eq!(parameters.max_votes, 2);
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

    // Vote 'Yes' with user 1
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

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let mut voters = HashMap::new();
    voters.insert(USER_1.participant_id, VoteOption::Yes);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        results: outgoing::Results {
            votes: Votes {
                yes: 1,
                no: 0,
                abstain: None,
            },
            voters: voters.clone(),
        },
    }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    // Vote 'No' with user 2
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

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    voters.insert(USER_2.participant_id, VoteOption::No);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        results: outgoing::Results {
            votes: Votes {
                yes: 1,
                no: 1,
                abstain: None,
            },
            voters: voters.clone(),
        },
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
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            vote_id,
            kind: rabbitmq::StopKind::ByParticipant(USER_1.participant_id),
            results: outgoing::FinalResults::Valid(outgoing::Results {
                votes: Votes {
                    yes: 1,
                    no: 1,
                    abstain: None,
                },
                voters: voters.clone(),
            }),
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
        assert_eq!(protocol.len(), 5);
    }
}

#[actix_rt::test]
#[serial]
async fn basic_vote_abstain() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: true,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.max_votes, 2);

        parameters.vote_id
    } else {
        panic!("Expected Start message")
    };

    // Expect Start response in websocket for user 2
    if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.vote_id, vote_id);
        assert_eq!(parameters.max_votes, 2);
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

    // Vote 'Abstain' with user 1
    let vote_abstain = incoming::Message::Vote(VoteMessage {
        vote_id,
        option: VoteOption::Abstain,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, vote_abstain)
        .unwrap();

    //Expect VoteSuccess
    let vote_response = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Abstain,
                issuer: USER_1.participant_id,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let mut voters = HashMap::new();
    voters.insert(USER_1.participant_id, VoteOption::Abstain);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        results: outgoing::Results {
            votes: Votes {
                yes: 0,
                no: 0,
                abstain: Some(1),
            },
            voters: voters.clone(),
        },
    }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    // Vote 'No' with user 2
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

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    voters.insert(USER_2.participant_id, VoteOption::No);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        results: outgoing::Results {
            votes: Votes {
                yes: 0,
                no: 1,
                abstain: Some(1),
            },
            voters: voters.clone(),
        },
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
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            vote_id,
            kind: rabbitmq::StopKind::ByParticipant(USER_1.participant_id),
            results: outgoing::FinalResults::Valid(outgoing::Results {
                votes: Votes {
                    yes: 0,
                    no: 1,
                    abstain: Some(1),
                },
                voters: voters.clone(),
            }),
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
        assert_eq!(protocol.len(), 5);
    }
}

#[actix_rt::test]
#[serial]
async fn expired_vote() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: Some(5),
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.max_votes, 2);

        parameters.vote_id
    } else {
        panic!("Expected Start message")
    };

    // Expect Start response in websocket for user 2
    if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.vote_id, vote_id);
        assert_eq!(parameters.max_votes, 2);
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

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            vote_id,
            kind: rabbitmq::StopKind::Expired,
            results: outgoing::FinalResults::Valid(outgoing::Results {
                votes: Votes {
                    yes: 0,
                    no: 0,
                    abstain: None,
                },
                voters: HashMap::new(),
            }),
        }));

    // receive expired stop message on user 1
    let stop_message = module_tester
        .receive_ws_message_override_timeout(&USER_1.participant_id, Duration::from_secs(6))
        .await
        .expect("Didn't receive stop message after 5 seconds, vote should have expired");

    assert_eq!(expected_stop_message, stop_message);

    // receive expired stop message on user 2
    let stop_message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .expect("Didn't receive stop message for user 2");

    assert_eq!(expected_stop_message, stop_message);

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
        assert_eq!(protocol.len(), 3);
    }
}

#[actix_rt::test]
#[serial]
async fn auto_stop_vote() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_stop: true,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.max_votes, 2);

        parameters.vote_id
    } else {
        panic!("Expected Start message")
    };

    // Expect Start response in websocket for user 2
    if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.vote_id, vote_id);
        assert_eq!(parameters.max_votes, 2);
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

    // Vote 'Yes' with user 1
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

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let votes = Votes {
        yes: 1,
        no: 0,
        abstain: None,
    };

    let mut voters = HashMap::new();
    voters.insert(USER_1.participant_id, VoteOption::Yes);

    let results = outgoing::Results {
        votes,
        voters: voters.clone(),
    };

    let expected_update =
        WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults { vote_id, results }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    // Vote 'No' with user 2 (auto stop should happen here)
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

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    // Expect a vote Update message on all participants
    let votes = Votes {
        yes: 1,
        no: 1,
        abstain: None,
    };

    voters.insert(USER_2.participant_id, VoteOption::No);

    let results = outgoing::Results {
        votes,
        voters: voters.clone(),
    };

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        results: results.clone(),
    }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    let final_results = outgoing::FinalResults::Valid(results);

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            vote_id,
            kind: rabbitmq::StopKind::Auto,
            results: final_results,
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
        assert_eq!(protocol.len(), 5);
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn start_with_one_participant() {
    let test_ctx = TestContext::new().await;
    let module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn start_with_empty_participants() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    let expected_response = WsMessageOutgoing::Module(outgoing::Message::Error(
        ErrorKind::BadRequest(InvalidFields {
            fields: vec!["allowed_participants".into()],
        }),
    ));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_response, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn initiator_left() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    default_start_setup(&mut module_tester).await;

    //ignore start message for user 1
    let _ = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    // leave with user 1
    module_tester.leave(&USER_1.participant_id).await.unwrap();

    // receive cancel on user 2
    let initiator_left_cancel = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Canceled(Canceled { vote_id: _, reason })) =
        initiator_left_cancel
    {
        assert_eq!(reason, CancelReason::InitiatorLeft);
    } else {
        panic!("Expected cancel due to initiator leaving")
    }

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn ineligible_voter() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters),
        )
        .unwrap();

    let vote_id = receive_start_on_user2(&mut module_tester).await;

    // try to vote with ineligible user 2
    module_tester
        .send_ws_message(
            &USER_2.participant_id,
            incoming::Message::Vote(VoteMessage {
                vote_id,
                option: VoteOption::Yes,
            }),
        )
        .unwrap();

    // expect the vote to fail due to the user being ineligible
    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Failed(VoteFailed::Ineligible),
        }));

    let message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_vote_response, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn start_with_allowed_guest() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // start the vote with a guest as an allowed participant
    let guest = ParticipantId::new_test(11311);

    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, guest, USER_2.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    // start vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    let expected_error = WsMessageOutgoing::Module(outgoing::Message::Error(
        ErrorKind::AllowlistContainsGuests(GuestParticipants {
            guests: vec![guest],
        }),
    ));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_error, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn vote_on_nonexistent_vote() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let vote_id = VoteId::from(Uuid::from_u128(11311));

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Vote(VoteMessage {
                vote_id,
                option: VoteOption::Yes,
            }),
        )
        .unwrap();

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Failed(VoteFailed::InvalidVoteId),
        }));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_vote_response, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn vote_on_completed_vote() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let vote_id = default_start_setup(&mut module_tester).await;

    // stop vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Stop(incoming::Stop { vote_id }),
        )
        .unwrap();

    // expect vote stop
    if let WsMessageOutgoing::Module(outgoing::Message::Stopped(Stopped { .. })) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        // seems good
    } else {
        panic!("Expected stop message")
    };

    // try to vote with user 2
    module_tester
        .send_ws_message(
            &USER_2.participant_id,
            incoming::Message::Vote(VoteMessage {
                vote_id,
                option: VoteOption::Yes,
            }),
        )
        .unwrap();

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Failed(VoteFailed::InvalidVoteId),
        }));

    let message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_vote_response, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn vote_twice() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    // start vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // receive start vote on user 1
    let vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(Parameters {
        initiator_id: _,
        vote_id,
        ..
    })) = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap()
    {
        vote_id
    } else {
        panic!("Expected started message")
    };

    // vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Vote(VoteMessage {
                vote_id,
                option: VoteOption::Yes,
            }),
        )
        .unwrap();

    let expected_success_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
            }),
        }));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_success_vote_response, message);

    let mut voters = HashMap::new();
    voters.insert(USER_1.participant_id, VoteOption::Yes);
    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        vote_id,
        results: outgoing::Results {
            votes: Votes {
                yes: 1,
                no: 0,
                abstain: None,
            },
            voters,
        },
    }));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_update, message);

    // vote again with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Vote(VoteMessage {
                vote_id,
                option: VoteOption::No,
            }),
        )
        .unwrap();

    let expected_failed_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            vote_id,
            response: Response::Failed(VoteFailed::Ineligible),
        }));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_failed_vote_response, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn ineligible_stop() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let vote_id = default_start_setup(&mut module_tester).await;

    // stop vote with user 2
    let stop_vote = incoming::Message::Stop(Stop { vote_id });

    module_tester
        .send_ws_message(&USER_2.participant_id, stop_vote)
        .unwrap();

    let expected_error_message =
        WsMessageOutgoing::Module(outgoing::Message::Error(ErrorKind::Ineligible));

    let message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_error_message, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn ineligible_cancel() {
    let test_ctx = TestContext::new().await;
    let mut module_tester = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let vote_id = default_start_setup(&mut module_tester).await;

    // cancel vote with user 2
    let cancel_vote = incoming::Message::Cancel(Cancel {
        vote_id,
        reason: "Yes".into(),
    });

    module_tester
        .send_ws_message(&USER_2.participant_id, cancel_vote)
        .unwrap();

    let expected_error_message =
        WsMessageOutgoing::Module(outgoing::Message::Error(ErrorKind::Ineligible));

    let message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_error_message, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn join_as_guest() {
    let test_ctx = TestContext::new().await;
    test_ctx
        .db_ctx
        .create_test_user(USER_1.user_id, Vec::new())
        .unwrap();
    let guest = ParticipantId::new_test(11311);

    let room = test_ctx
        .db_ctx
        .create_test_room(ROOM_ID, USER_1.user_id)
        .unwrap();

    let mut module_tester = ModuleTester::<LegalVote>::new(
        test_ctx.db_ctx.db_conn.clone(),
        test_ctx.redis_conn.clone(),
        room,
    );

    // Join with guest
    let module = module_tester.join_guest(guest, "Guest", ()).await;
    if let Err(e) = module {
        assert!(e.is::<NoInitError>(), "Module initialized with guest");
    }
    module_tester.shutdown().await.unwrap()
}

/// Start a vote with user1 with default UserParameters
async fn default_vote_start(module_tester: &mut ModuleTester<LegalVote>) {
    let start_parameters = UserParameters {
        name: "TestVote".into(),
        topic: "Does the test work?".into(),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_stop: false,
        duration: None,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters),
        )
        .unwrap();
}

/// Receive the vote start on user2 and return the corresponding vote id
async fn receive_start_on_user2(module_tester: &mut ModuleTester<LegalVote>) -> VoteId {
    if let WsMessageOutgoing::Module(outgoing::Message::Started(Parameters {
        initiator_id: _,
        vote_id,
        ..
    })) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        vote_id
    } else {
        panic!("Expected started message")
    }
}

/// A default setup where user1 starts the vote and user2 receives the started response.
///
/// Note: This leaves the started response in the ws_receive buffer of user1.
async fn default_start_setup(module_tester: &mut ModuleTester<LegalVote>) -> VoteId {
    default_vote_start(module_tester).await;
    receive_start_on_user2(module_tester).await
}
