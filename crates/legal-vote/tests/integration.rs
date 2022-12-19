use controller::prelude::chrono::{DateTime, TimeZone, Utc};
use controller::prelude::control::outgoing::JoinSuccess;
use controller::prelude::*;
use controller::prelude::{ModuleTester, WsMessageOutgoing};
use controller_shared::ParticipantId;
use db_storage::legal_votes::types::{
    CancelReason, Parameters, Tally, Token, UserParameters, VoteKind, VoteOption,
};
use db_storage::legal_votes::{LegalVote as DbLegalVote, LegalVoteId};
use db_storage::users::User;
use k3k_legal_vote::incoming::{Stop, VoteMessage};
use k3k_legal_vote::outgoing::{
    ErrorKind, GuestParticipants, InvalidFields, Response, Results, Stopped, VoteFailed,
    VoteResponse, VoteResults, VoteSuccess, VotingRecord,
};
use k3k_legal_vote::rabbitmq;
use k3k_legal_vote::{
    frontend_data::{FrontendData, StopKind, VoteState, VoteSummary},
    incoming, outgoing, LegalVote,
};
use pretty_assertions::assert_eq;
use serde_json::Value;
use serial_test::serial;
use std::collections::HashMap;
use std::time::Duration;
use test_util::*;
use uuid::Uuid;

fn compare_stopped_message_except_for_timestamp(
    actual: WsMessageOutgoing<LegalVote>,
    mut expected: WsMessageOutgoing<LegalVote>,
) -> DateTime<Utc> {
    let timestamp = match actual {
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            end_time,
            ..
        })) => end_time,
        _ => panic!("Message type mismatch"),
    };
    match expected {
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            ref mut end_time,
            ..
        })) => *end_time = timestamp,
        _ => panic!("Message type mismatch"),
    };
    assert_eq!(actual, expected);
    timestamp
}

#[actix_rt::test]
#[serial]
async fn basic_vote_roll_call() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;
    let mut db_conn = test_ctx.db_ctx.db.get_conn().unwrap();

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let (legal_vote_id, user_1_token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            assert_eq!(parameters.initiator_id, USER_1.participant_id);
            assert_eq!(parameters.inner, start_parameters);
            assert_eq!(parameters.max_votes, 2);
            assert!(parameters.token.is_some());

            (parameters.legal_vote_id, parameters.token.unwrap())
        } else {
            panic!("Expected Start message")
        };

    // Expect Start response in websocket for user 2
    let user_2_token = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.legal_vote_id, legal_vote_id);
        assert_eq!(parameters.max_votes, 2);
        assert!(parameters.token.is_some());

        parameters.token.unwrap()
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `legal_vote_id` to exist in database
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    assert_eq!(protocol_entries, Value::Array(vec![]));

    // Start casting votes

    // Vote 'Yes' with user 1
    let vote_yes = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::Yes,
        token: user_1_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
                consumed_token: user_1_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let mut voters = HashMap::new();
    voters.insert(user1.id, VoteOption::Yes);

    let voting_record = VotingRecord::UserVotes(voters.clone());

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 0,
                abstain: None,
            },
            voting_record: Some(voting_record.clone()),
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
        legal_vote_id,
        option: VoteOption::No,
        token: user_2_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
                consumed_token: user_2_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    voters.insert(user2.id, VoteOption::No);

    let voting_record = VotingRecord::UserVotes(voters);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 1,
                abstain: None,
            },
            voting_record: Some(voting_record.clone()),
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
    let stop_vote = incoming::Message::Stop(Stop { legal_vote_id });

    module_tester
        .send_ws_message(&USER_1.participant_id, stop_vote)
        .unwrap();

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            legal_vote_id,
            kind: rabbitmq::StopKind::ByParticipant(USER_1.participant_id),
            results: outgoing::FinalResults::Valid(outgoing::Results {
                tally: Tally {
                    yes: 1,
                    no: 1,
                    abstain: None,
                },
                voting_record: Some(voting_record.clone()),
            }),
            end_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
        }));

    // expect stop messages for all users
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");

        compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());
    }

    // check the vote protocol
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    if let Value::Array(entries) = protocol_entries {
        assert_eq!(entries.len(), 5);
    }
}

#[actix_rt::test]
#[serial]
async fn basic_vote_pseudonymous() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;
    let mut db_conn = test_ctx.db_ctx.db.get_conn().unwrap();

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::Pseudonymous,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let (legal_vote_id, user_1_token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            assert_eq!(parameters.initiator_id, USER_1.participant_id);
            assert_eq!(parameters.inner, start_parameters);
            assert_eq!(parameters.max_votes, 2);
            assert!(parameters.token.is_some());

            (parameters.legal_vote_id, parameters.token.unwrap())
        } else {
            panic!("Expected Start message")
        };

    // Expect Start response in websocket for user 2
    let user_2_token = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.legal_vote_id, legal_vote_id);
        assert_eq!(parameters.max_votes, 2);
        assert!(parameters.token.is_some());

        parameters.token.unwrap()
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `legal_vote_id` to exist in database
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    assert_eq!(protocol_entries, Value::Array(vec![]));

    // Start casting votes

    // Vote 'Yes' with user 1
    let vote_yes = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::Yes,
        token: user_1_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
                consumed_token: user_1_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 0,
                abstain: None,
            },
            voting_record: None,
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
        legal_vote_id,
        option: VoteOption::No,
        token: user_2_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
                consumed_token: user_2_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 1,
                abstain: None,
            },
            voting_record: None,
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
    let stop_vote = incoming::Message::Stop(Stop { legal_vote_id });

    module_tester
        .send_ws_message(&USER_1.participant_id, stop_vote)
        .unwrap();

    let token_votes = HashMap::from_iter(vec![
        (user_1_token, VoteOption::Yes),
        (user_2_token, VoteOption::No),
    ]);

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            legal_vote_id,
            kind: rabbitmq::StopKind::ByParticipant(USER_1.participant_id),
            results: outgoing::FinalResults::Valid(outgoing::Results {
                tally: Tally {
                    yes: 1,
                    no: 1,
                    abstain: None,
                },
                voting_record: Some(VotingRecord::TokenVotes(token_votes)),
            }),
            end_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
        }));

    // expect stop messages for all users
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");

        compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());
    }

    // check the vote protocol
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    if let Value::Array(entries) = protocol_entries {
        assert_eq!(entries.len(), 5);
    }
}

#[actix_rt::test]
#[serial]
async fn hidden_legal_vote() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;
    let mut db_conn = test_ctx.db_ctx.db.get_conn().unwrap();

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::Pseudonymous,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let (legal_vote_id, user_1_token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            assert_eq!(parameters.initiator_id, USER_1.participant_id);
            assert_eq!(parameters.inner, start_parameters);
            assert_eq!(parameters.max_votes, 2);
            assert!(parameters.token.is_some());

            (parameters.legal_vote_id, parameters.token.unwrap())
        } else {
            panic!("Expected Start message")
        };

    // Expect Start response in websocket for user 2
    let user_2_token = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.legal_vote_id, legal_vote_id);
        assert_eq!(parameters.max_votes, 2);
        assert!(parameters.token.is_some());

        parameters.token.unwrap()
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `legal_vote_id` to exist in database
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    assert_eq!(protocol_entries, Value::Array(vec![]));

    // Start casting votes

    // Vote 'Yes' with user 1
    let vote_yes = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::Yes,
        token: user_1_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
                consumed_token: user_1_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 0,
                abstain: None,
            },
            voting_record: None,
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
        legal_vote_id,
        option: VoteOption::No,
        token: user_2_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
                consumed_token: user_2_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 1,
                abstain: None,
            },
            voting_record: None,
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
    let stop_vote = incoming::Message::Stop(Stop { legal_vote_id });

    module_tester
        .send_ws_message(&USER_1.participant_id, stop_vote)
        .unwrap();

    let token_votes = HashMap::from_iter(vec![
        (user_1_token, VoteOption::Yes),
        (user_2_token, VoteOption::No),
    ]);

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            legal_vote_id,
            kind: rabbitmq::StopKind::ByParticipant(USER_1.participant_id),
            results: outgoing::FinalResults::Valid(outgoing::Results {
                tally: Tally {
                    yes: 1,
                    no: 1,
                    abstain: None,
                },
                voting_record: Some(VotingRecord::TokenVotes(token_votes)),
            }),
            end_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
        }));

    // expect stop messages for all users
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");

        compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());
    }

    // check the vote protocol
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    // TODO: parse and check the vote entries
    let protocol_entries = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    if let Value::Array(entries) = protocol_entries {
        assert_eq!(entries.len(), 5);
    }
}

#[actix_rt::test]
#[serial]
async fn basic_vote_abstain() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;
    let mut db_conn = test_ctx.db_ctx.db.get_conn().unwrap();

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: true,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let (legal_vote_id, user_1_token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            assert_eq!(parameters.initiator_id, USER_1.participant_id);
            assert_eq!(parameters.inner, start_parameters);
            assert_eq!(parameters.max_votes, 2);
            assert!(parameters.token.is_some());

            (parameters.legal_vote_id, parameters.token.unwrap())
        } else {
            panic!("Expected Start message")
        };

    // Expect Start response in websocket for user 2
    let user_2_token = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.legal_vote_id, legal_vote_id);
        assert_eq!(parameters.max_votes, 2);
        assert!(parameters.token.is_some());

        parameters.token.unwrap()
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `legal_vote_id` to exist in database
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    assert_eq!(protocol_entries, Value::Array(vec![]));

    // Start casting votes

    // Vote 'Abstain' with user 1
    let vote_abstain = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::Abstain,
        token: user_1_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Abstain,
                issuer: USER_1.participant_id,
                consumed_token: user_1_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let mut voters = HashMap::new();
    voters.insert(user1.id, VoteOption::Abstain);

    let voting_record = VotingRecord::UserVotes(voters.clone());

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 0,
                no: 0,
                abstain: Some(1),
            },
            voting_record: Some(voting_record.clone()),
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
        legal_vote_id,
        option: VoteOption::No,
        token: user_2_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
                consumed_token: user_2_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    voters.insert(user2.id, VoteOption::No);

    let voting_record = VotingRecord::UserVotes(voters);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 0,
                no: 1,
                abstain: Some(1),
            },
            voting_record: Some(voting_record.clone()),
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
    let stop_vote = incoming::Message::Stop(Stop { legal_vote_id });

    module_tester
        .send_ws_message(&USER_1.participant_id, stop_vote)
        .unwrap();

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            legal_vote_id,
            kind: rabbitmq::StopKind::ByParticipant(USER_1.participant_id),
            results: outgoing::FinalResults::Valid(outgoing::Results {
                tally: Tally {
                    yes: 0,
                    no: 1,
                    abstain: Some(1),
                },
                voting_record: Some(voting_record),
            }),
            end_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
        }));

    // expect stop messages for all users
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");

        compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());
    }

    // check the vote protocol
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    if let Value::Array(protocol) = protocol_entries {
        assert_eq!(protocol.len(), 5);
    }
}

#[actix_rt::test]
#[serial]
async fn expired_vote() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;
    let mut db_conn = test_ctx.db_ctx.db.get_conn().unwrap();

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: Some(5),
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let legal_vote_id = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.max_votes, 2);

        parameters.legal_vote_id
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
        assert_eq!(parameters.legal_vote_id, legal_vote_id);
        assert_eq!(parameters.max_votes, 2);
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `legal_vote_id` to exist in database
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    assert_eq!(protocol_entries, Value::Array(vec![]));

    let expected_stop_message =
        WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            legal_vote_id,
            kind: rabbitmq::StopKind::Expired,
            results: outgoing::FinalResults::Valid(outgoing::Results {
                tally: Tally {
                    yes: 0,
                    no: 0,
                    abstain: None,
                },
                voting_record: Some(VotingRecord::UserVotes(HashMap::new())),
            }),
            end_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
        }));

    // receive expired stop message on user 1
    let stop_message = module_tester
        .receive_ws_message_override_timeout(&USER_1.participant_id, Duration::from_secs(6))
        .await
        .expect("Didn't receive stop message after 5 seconds, vote should have expired");

    compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());

    // receive expired stop message on user 2
    let stop_message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .expect("Didn't receive stop message for user 2");

    compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());

    // check the vote protocol
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    if let Value::Array(entries) = protocol_entries {
        assert_eq!(entries.len(), 3);
    }
}

#[actix_rt::test]
#[serial]
async fn auto_stop_vote() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;
    let mut db_conn = test_ctx.db_ctx.db.get_conn().unwrap();

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: true,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let (legal_vote_id, user_1_token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            assert_eq!(parameters.initiator_id, USER_1.participant_id);
            assert_eq!(parameters.inner, start_parameters);
            assert_eq!(parameters.max_votes, 2);
            assert!(parameters.token.is_some());

            (parameters.legal_vote_id, parameters.token.unwrap())
        } else {
            panic!("Expected Start message")
        };

    // Expect Start response in websocket for user 2
    let user_2_token = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap()
    {
        assert_eq!(parameters.initiator_id, USER_1.participant_id);
        assert_eq!(parameters.inner, start_parameters);
        assert_eq!(parameters.legal_vote_id, legal_vote_id);
        assert_eq!(parameters.max_votes, 2);
        assert!(parameters.token.is_some());

        parameters.token.unwrap()
    } else {
        panic!("Expected Start message")
    };

    // Expect a empty legal_vote with `legal_vote_id` to exist in database
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    assert_eq!(protocol_entries, Value::Array(vec![]));

    // Start casting votes

    // Vote 'Yes' with user 1
    let vote_yes = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::Yes,
        token: user_1_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
                consumed_token: user_1_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let tally = Tally {
        yes: 1,
        no: 0,
        abstain: None,
    };

    let mut voters = HashMap::new();
    voters.insert(user1.id, VoteOption::Yes);

    let voting_record = VotingRecord::UserVotes(voters.clone());

    let results = outgoing::Results {
        tally,
        voting_record: Some(voting_record),
    };

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results,
    }));

    for user in USERS {
        let update = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();

        assert_eq!(expected_update, update);
    }

    // Vote 'No' with user 2 (auto stop should happen here)
    let vote_no = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::No,
        token: user_2_token,
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
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::No,
                issuer: USER_2.participant_id,
                consumed_token: user_2_token,
            }),
        }));

    assert_eq!(expected_vote_response, vote_response);

    // Expect a vote Update message on all participants
    let tally = Tally {
        yes: 1,
        no: 1,
        abstain: None,
    };

    voters.insert(user2.id, VoteOption::No);

    let voting_record = VotingRecord::UserVotes(voters);

    let results = outgoing::Results {
        tally,
        voting_record: Some(voting_record.clone()),
    };

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
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
            legal_vote_id,
            kind: rabbitmq::StopKind::Auto,
            results: final_results,
            end_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
        }));

    // expect stop messages for all users
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");

        compare_stopped_message_except_for_timestamp(stop_message, expected_stop_message.clone());
    }

    // check the vote protocol
    let legal_vote = DbLegalVote::get(&mut db_conn, legal_vote_id).unwrap();

    assert_eq!(legal_vote.id, legal_vote_id);
    assert_eq!(legal_vote.created_by, user1.id);

    let protocol_entries: Value = serde_json::from_str(legal_vote.protocol.entries.get()).unwrap();
    if let Value::Array(entries) = protocol_entries {
        assert_eq!(entries.len(), 5);
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn start_with_one_participant() {
    let test_ctx = TestContext::new().await;
    let (module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
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
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
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
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    default_start_setup(&mut module_tester).await;

    // leave with user 1
    module_tester.leave(&USER_1.participant_id).await.unwrap();

    // receive cancel on user 2
    let initiator_left_cancel = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Canceled(rabbitmq::Canceled {
        legal_vote_id: _,
        reason,
        end_time: _,
    })) = initiator_left_cancel
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
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters),
        )
        .unwrap();

    let (legal_vote_id, token) = receive_start_on_user2(&mut module_tester).await;
    assert!(token.is_none());

    // try to vote with ineligible user 2
    module_tester
        .send_ws_message(
            &USER_2.participant_id,
            incoming::Message::Vote(VoteMessage {
                legal_vote_id,
                option: VoteOption::Yes,
                token: Token::default(),
            }),
        )
        .unwrap();

    // expect the vote to fail due to the user being ineligible
    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            legal_vote_id,
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
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    // start the vote with a guest as an allowed participant
    let guest = ParticipantId::new_test(11311);

    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, guest, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
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
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let legal_vote_id = LegalVoteId::from(Uuid::from_u128(11311));

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Vote(VoteMessage {
                legal_vote_id,
                option: VoteOption::Yes,
                token: Token::new(0),
            }),
        )
        .unwrap();

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            legal_vote_id,
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
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let (legal_vote_id, tokens) = default_start_setup(&mut module_tester).await;

    // stop vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Stop(incoming::Stop { legal_vote_id }),
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
                legal_vote_id,
                option: VoteOption::Yes,
                token: tokens[1].unwrap(),
            }),
        )
        .unwrap();

    let expected_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            legal_vote_id,
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
    let (mut module_tester, user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    // start vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // receive start vote on user 1
    let (legal_vote_id, token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(Parameters {
            token,
            legal_vote_id,
            ..
        })) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            (legal_vote_id, token.unwrap())
        } else {
            panic!("Expected started message")
        };

    // vote with user 1
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Vote(VoteMessage {
                legal_vote_id,
                option: VoteOption::Yes,
                token,
            }),
        )
        .unwrap();

    let expected_success_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            legal_vote_id,
            response: Response::Success(VoteSuccess {
                vote_option: VoteOption::Yes,
                issuer: USER_1.participant_id,
                consumed_token: token,
            }),
        }));

    let message = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_success_vote_response, message);

    let mut voters = HashMap::new();
    voters.insert(user1.id, VoteOption::Yes);

    let voting_record = VotingRecord::UserVotes(voters);

    let expected_update = WsMessageOutgoing::Module(outgoing::Message::Updated(VoteResults {
        legal_vote_id,
        results: outgoing::Results {
            tally: Tally {
                yes: 1,
                no: 0,
                abstain: None,
            },
            voting_record: Some(voting_record),
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
                legal_vote_id,
                option: VoteOption::No,
                token,
            }),
        )
        .unwrap();

    let expected_failed_vote_response =
        WsMessageOutgoing::Module(outgoing::Message::Voted(VoteResponse {
            legal_vote_id,
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
async fn non_moderator_stop() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let (legal_vote_id, _) = default_start_setup(&mut module_tester).await;

    // stop vote with user 2
    let stop_vote = incoming::Message::Stop(Stop { legal_vote_id });

    module_tester
        .send_ws_message(&USER_2.participant_id, stop_vote)
        .unwrap();

    let expected_error_message =
        WsMessageOutgoing::Module(outgoing::Message::Error(ErrorKind::InsufficentPermissions));

    let message = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(expected_error_message, message);

    module_tester.shutdown().await.unwrap()
}

#[actix_rt::test]
#[serial]
async fn non_moderator_cancel() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    let (legal_vote_id, _) = default_start_setup(&mut module_tester).await;

    // cancel vote with user 2
    let cancel_vote = incoming::Message::Cancel(incoming::Cancel {
        legal_vote_id,
        reason: "Yes".into(),
    });

    module_tester
        .send_ws_message(&USER_2.participant_id, cancel_vote)
        .unwrap();

    let expected_error_message =
        WsMessageOutgoing::Module(outgoing::Message::Error(ErrorKind::InsufficentPermissions));

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
    let user1 = test_ctx
        .db_ctx
        .create_test_user(USER_1.n, Vec::new())
        .unwrap();
    let guest = ParticipantId::new_test(11311);

    let waiting_room = false;
    let room = test_ctx
        .db_ctx
        .create_test_room(ROOM_ID, user1.id, waiting_room)
        .unwrap();

    let mut module_tester = ModuleTester::<LegalVote>::new(
        test_ctx.db_ctx.db.clone(),
        test_ctx.authz.clone(),
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

#[actix_rt::test]
#[serial]
async fn frontend_data() {
    async fn check_user_join_module_data(
        module_tester: &mut ModuleTester<LegalVote>,
        user3: User,
        frontend_data: FrontendData,
    ) {
        // Join with user3
        module_tester
            .join_user(
                USER_3.participant_id,
                user3.clone(),
                Role::User,
                USER_3.name,
                (),
            )
            .await
            .unwrap();

        // Ignore join message
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap();
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap();

        if let WsMessageOutgoing::Control(control::outgoing::Message::JoinSuccess(JoinSuccess {
            module_data,
            ..
        })) = module_tester
            .receive_ws_message(&USER_3.participant_id)
            .await
            .unwrap()
        {
            assert!(module_data.contains_key(LegalVote::NAMESPACE));
            assert_eq!(
                serde_json::to_value(frontend_data).unwrap(),
                *module_data.get(LegalVote::NAMESPACE).unwrap(),
            );
        } else {
            panic!("Expected JoinSuccess Message")
        }

        module_tester.leave(&USER_3.participant_id).await.unwrap();

        // Ignore leave message
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap();
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap();
    }

    let test_ctx = TestContext::new().await;
    let (mut module_tester, user1, user2) = common::setup_users::<LegalVote>(&test_ctx, ()).await;

    const USER_3: TestUser = TestUser {
        n: 3,
        participant_id: ParticipantId::new_test(3),
        name: "user3",
    };
    let user3 = test_ctx.db_ctx.create_test_user(USER_3.n, vec![]).unwrap();

    // Expect empty votes on join
    check_user_join_module_data(
        &mut module_tester,
        user3.clone(),
        FrontendData { votes: vec![] },
    )
    .await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: true,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Start response in websocket for user 1
    let (parameters, user_1_token) =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) = module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
        {
            let token = parameters.token.unwrap();
            (parameters, token)
        } else {
            panic!("Expected Start message")
        };

    // Expect Start response in websocket for user 2
    let user_2_token = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_2.participant_id)
            .await
            .unwrap()
    {
        parameters.token.unwrap()
    } else {
        panic!("Expected Start message")
    };

    // Check frontend_data
    {
        let mut parameters = parameters.clone();
        parameters.token = None;
        check_user_join_module_data(
            &mut module_tester,
            user3.clone(),
            FrontendData {
                votes: vec![VoteSummary {
                    parameters,
                    state: VoteState::Started,
                    end_time: None,
                }],
            },
        )
        .await;
    }

    let legal_vote_id = parameters.legal_vote_id;

    // Start casting votes

    // Vote 'Yes' with user 1
    let vote_yes = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::Yes,
        token: user_1_token,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, vote_yes)
        .unwrap();

    // Ignore VoteSuccess
    module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    // Ignore a vote Update message on all participants
    for user in USERS {
        module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();
    }

    // Vote 'No' with user 2 (auto stop should happen here)
    let vote_no = incoming::Message::Vote(VoteMessage {
        legal_vote_id,
        option: VoteOption::No,
        token: user_2_token,
    });

    module_tester
        .send_ws_message(&USER_2.participant_id, vote_no)
        .unwrap();

    // Ignore VoteSuccess
    module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    // Ignore a vote Update message on all participants
    for user in USERS {
        module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .unwrap();
    }

    // Check stop messages for all users and extract timestamp
    let mut timestamp = None;
    for user in USERS {
        let stop_message = module_tester
            .receive_ws_message(&user.participant_id)
            .await
            .expect("Expected stop message");
        if let WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
            end_time,
            ..
        })) = stop_message
        {
            timestamp = Some(end_time);
        } else {
            panic!("Stop message expected");
        }
    }
    assert!(timestamp.is_some());

    let vote_1_summary = {
        let mut parameters = parameters.clone();
        parameters.token = None;
        VoteSummary {
            parameters,
            state: VoteState::Finished {
                stop_kind: StopKind::Auto,
                results: Results {
                    tally: Tally {
                        yes: 1,
                        no: 1,
                        abstain: None,
                    },
                    voting_record: Some(VotingRecord::UserVotes(HashMap::from_iter([
                        (user1.id, VoteOption::Yes),
                        (user2.id, VoteOption::No),
                    ]))),
                },
            },
            end_time: timestamp,
        }
    };

    // Check frontend_data upon user 3 rejoin
    check_user_join_module_data(
        &mut module_tester,
        user3.clone(),
        FrontendData {
            votes: vec![vote_1_summary.clone()],
        },
    )
    .await;

    // Start legal vote as user 1
    let start_parameters = UserParameters {
        kind: VoteKind::Pseudonymous,
        name: "TestVote pseudonymous".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: true,
        auto_close: true,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters.clone()),
        )
        .unwrap();

    // Expect Started event in websocket for user 1
    let parameters = if let WsMessageOutgoing::Module(outgoing::Message::Started(parameters)) =
        module_tester
            .receive_ws_message(&USER_1.participant_id)
            .await
            .unwrap()
    {
        parameters
    } else {
        panic!("Expected Start message")
    };

    // Expect Start event in websocket for user 2
    if let WsMessageOutgoing::Module(outgoing::Message::Started(_)) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
    } else {
        panic!("Expected Start message")
    };

    // Check frontend_data
    {
        let mut parameters = parameters.clone();
        parameters.token = None;
        check_user_join_module_data(
            &mut module_tester,
            user3.clone(),
            FrontendData {
                votes: vec![
                    vote_1_summary,
                    VoteSummary {
                        parameters,
                        state: VoteState::Started,
                        end_time: None,
                    },
                ],
            },
        )
        .await;
    }

    module_tester.shutdown().await.unwrap();
}

/// Start a vote with user1 with default UserParameters
async fn default_vote_start_by_user1(
    module_tester: &mut ModuleTester<LegalVote>,
) -> (LegalVoteId, Option<Token>) {
    let start_parameters = UserParameters {
        kind: VoteKind::RollCall,
        name: "TestVote".into(),
        subtitle: Some("A subtitle".into()),
        topic: Some("Does the test work?".into()),
        allowed_participants: vec![USER_1.participant_id, USER_2.participant_id],
        enable_abstain: false,
        auto_close: false,
        duration: None,
        create_pdf: false,
    };

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(start_parameters),
        )
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Started(Parameters {
        token,
        legal_vote_id,
        ..
    })) = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap()
    {
        (legal_vote_id, token)
    } else {
        panic!("Expected started message")
    }
}

/// Receive the vote start on user2 and return the corresponding vote id
async fn receive_start_on_user2(
    module_tester: &mut ModuleTester<LegalVote>,
) -> (LegalVoteId, Option<Token>) {
    if let WsMessageOutgoing::Module(outgoing::Message::Started(Parameters {
        token,
        legal_vote_id,
        ..
    })) = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap()
    {
        (legal_vote_id, token)
    } else {
        panic!("Expected started message")
    }
}

/// A default setup where user1 starts the vote and user2 receives the started response.
///
/// Returns a tuple with the first element being the legal vote id, and the secend element
/// being a vector containing the tokens for user 1 and user 2.
async fn default_start_setup(
    module_tester: &mut ModuleTester<LegalVote>,
) -> (LegalVoteId, Vec<Option<Token>>) {
    let (legal_vote_id, user_1_token) = default_vote_start_by_user1(module_tester).await;
    let (_, user_2_token) = receive_start_on_user2(module_tester).await;
    (legal_vote_id, vec![user_1_token, user_2_token])
}
