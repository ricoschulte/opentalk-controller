use controller::prelude::*;
use k3k_polls::*;
use serial_test::serial;
use std::time::Duration;
use test_util::*;

async fn start_poll(module_tester: &mut ModuleTester<Polls>, live_poll: bool) -> outgoing::Started {
    let start = incoming::Message::Start(incoming::Start {
        topic: "polling".into(),
        live: live_poll,
        choices: vec!["yes".into(), "no".into()],
        duration: Duration::from_secs(2),
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, start)
        .unwrap();

    let started1 = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    let started2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(started1, started2);

    if let WsMessageOutgoing::Module(outgoing::Message::Started(outgoing::Started {
        id,
        topic,
        live,
        choices,
        duration,
    })) = started1
    {
        assert_eq!(topic, "polling");
        assert_eq!(live, live_poll);
        assert_eq!(
            choices,
            &[
                Choice {
                    id: ChoiceId(0),
                    content: "yes".into()
                },
                Choice {
                    id: ChoiceId(1),
                    content: "no".into()
                }
            ]
        );
        assert_eq!(duration.as_millis(), 2000);

        outgoing::Started {
            id,
            topic,
            live,
            choices,
            duration,
        }
    } else {
        panic!("unexpected {:?}", started1)
    }
}

#[actix_rt::test]
#[serial]
async fn full_poll_with_2sec_duration() {
    let test_ctx = TestContext::new().await;

    let mut module_tester = common::setup_users::<Polls>(&test_ctx, ()).await;

    let started = start_poll(&mut module_tester, true).await;

    // User 1 vote yes
    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Vote(incoming::Vote {
                poll_id: started.id,
                choice_id: ChoiceId(0),
            }),
        )
        .unwrap();

    let update1 = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    let update2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(update1, update2);

    if let WsMessageOutgoing::Module(outgoing::Message::LiveUpdate(outgoing::Results {
        id,
        results,
    })) = update1
    {
        assert_eq!(id, started.id);
        assert_eq!(
            results,
            &[
                outgoing::Item {
                    id: ChoiceId(0),
                    count: 1,
                },
                outgoing::Item {
                    id: ChoiceId(1),
                    count: 0
                }
            ]
        );
    } else {
        panic!("unexpected {:?}", update1)
    }

    // User 2 vote

    module_tester
        .send_ws_message(
            &USER_2.participant_id,
            incoming::Message::Vote(incoming::Vote {
                poll_id: started.id,
                choice_id: ChoiceId(1),
            }),
        )
        .unwrap();

    let update1 = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    let update2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(update1, update2);

    if let WsMessageOutgoing::Module(outgoing::Message::LiveUpdate(outgoing::Results {
        id,
        results,
    })) = update1
    {
        assert_eq!(id, started.id);
        assert_eq!(
            results,
            &[
                outgoing::Item {
                    id: ChoiceId(0),
                    count: 1,
                },
                outgoing::Item {
                    id: ChoiceId(1),
                    count: 1
                }
            ]
        );
    } else {
        panic!("unexpected {:?}", update1)
    }

    // User 2 vote again but fails

    module_tester
        .send_ws_message(
            &USER_2.participant_id,
            incoming::Message::Vote(incoming::Vote {
                poll_id: started.id,
                choice_id: ChoiceId(0),
            }),
        )
        .unwrap();

    let error = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Error(outgoing::Error::VotedAlready)) =
        error
    {
        // OK
    } else {
        panic!("unexpected {:?}", error)
    }

    // Poll expired, getting results in `Done` event

    let done1 = module_tester
        .receive_ws_message_override_timeout(&USER_1.participant_id, Duration::from_secs(3))
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Done(outgoing::Results {
        //
        id,
        results,
    })) = &done1
    {
        assert_eq!(*id, started.id);
        assert_eq!(
            results,
            &[
                outgoing::Item {
                    id: ChoiceId(0),
                    count: 1,
                },
                outgoing::Item {
                    id: ChoiceId(1),
                    count: 1,
                }
            ]
        );
    } else {
        panic!("unexpected {:?}", done1)
    }

    let done2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(done1, done2);

    module_tester.shutdown().await.unwrap()
}
