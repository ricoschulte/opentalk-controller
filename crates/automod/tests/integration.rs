use automod::config;
use automod::incoming;
use automod::outgoing;
use controller::prelude::*;
use controller_shared::ParticipantId;
use k3k_automod as automod;
use serial_test::serial;
use test_util::*;

#[actix_rt::test]
#[serial]
async fn reject_start_empty_allow_or_playlist() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) =
        common::setup_users::<automod::AutoMod>(&test_ctx, ()).await;

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(incoming::Start {
                parameter: config::Parameter {
                    selection_strategy: config::SelectionStrategy::Random,
                    show_list: true,
                    consider_hand_raise: false,
                    time_limit: None,
                    allow_double_selection: false,
                    animation_on_random: true,
                },
                allow_list: vec![],
                playlist: vec![],
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Error(outgoing::Error::InvalidSelection)) =
        answer
    {
        // yay
    } else {
        panic!()
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn reject_start_invalid_allow_list() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) =
        common::setup_users::<automod::AutoMod>(&test_ctx, ()).await;

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(incoming::Start {
                parameter: config::Parameter {
                    selection_strategy: config::SelectionStrategy::Random,
                    show_list: true,
                    consider_hand_raise: false,
                    time_limit: None,
                    allow_double_selection: false,
                    animation_on_random: true,
                },
                // Add the invalid user
                allow_list: vec![
                    ParticipantId::new_test(123457890),
                    ParticipantId::new_test(978123987234),
                ],
                playlist: vec![],
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Error(outgoing::Error::InvalidSelection)) =
        answer
    {
        // yay
    } else {
        panic!()
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn reject_start_invalid_allow_list_with_some_correct() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) =
        common::setup_users::<automod::AutoMod>(&test_ctx, ()).await;

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(incoming::Start {
                parameter: config::Parameter {
                    selection_strategy: config::SelectionStrategy::Random,
                    show_list: true,
                    consider_hand_raise: false,
                    time_limit: None,
                    allow_double_selection: false,
                    animation_on_random: true,
                },
                // Add the invalid user
                allow_list: vec![
                    USER_1.participant_id,
                    USER_2.participant_id,
                    ParticipantId::new_test(123457890),
                    ParticipantId::new_test(978123987234),
                ],
                playlist: vec![],
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Error(outgoing::Error::InvalidSelection)) =
        answer
    {
        // yay
    } else {
        panic!()
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn accept_valid_edit() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) =
        common::setup_users::<automod::AutoMod>(&test_ctx, ()).await;

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(incoming::Start {
                parameter: config::Parameter {
                    selection_strategy: config::SelectionStrategy::Random,
                    show_list: true,
                    consider_hand_raise: false,
                    time_limit: None,
                    allow_double_selection: false,
                    animation_on_random: true,
                },
                // Add valid users
                allow_list: vec![USER_1.participant_id, USER_2.participant_id],
                playlist: vec![],
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Started(_)) = answer {
        // ok
    } else {
        panic!()
    }

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Edit(incoming::Edit {
                allow_list: Some(vec![USER_1.participant_id]),
                playlist: None,
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::RemainingUpdated(
        outgoing::RemainingUpdated { remaining },
    )) = answer
    {
        assert_eq!(remaining, &[USER_1.participant_id]);
    } else {
        panic!()
    }

    module_tester.shutdown().await.unwrap();
}

#[actix_rt::test]
#[serial]
async fn reject_invalid_edit() {
    let test_ctx = TestContext::new().await;
    let (mut module_tester, _user1, _user2) =
        common::setup_users::<automod::AutoMod>(&test_ctx, ()).await;

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Start(incoming::Start {
                parameter: config::Parameter {
                    selection_strategy: config::SelectionStrategy::Random,
                    show_list: true,
                    consider_hand_raise: false,
                    time_limit: None,
                    allow_double_selection: false,
                    animation_on_random: true,
                },
                // Add valid users
                allow_list: vec![USER_1.participant_id, USER_2.participant_id],
                playlist: vec![],
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Started(_)) = answer {
        // ok
    } else {
        panic!()
    }

    module_tester
        .send_ws_message(
            &USER_1.participant_id,
            incoming::Message::Edit(incoming::Edit {
                allow_list: Some(vec![
                    USER_1.participant_id,
                    ParticipantId::new_test(978653421),
                ]),
                playlist: None,
            }),
        )
        .unwrap();

    let answer = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Error(outgoing::Error::InvalidSelection)) =
        answer
    {
        // yay
    } else {
        panic!()
    }

    module_tester.shutdown().await.unwrap();
}
