use controller::prelude::ModuleTester;
use controller::prelude::WsMessageOutgoing;
use k3k_timer::incoming;
use k3k_timer::incoming::Stop;
use k3k_timer::outgoing;
use k3k_timer::outgoing::StopKind;
use k3k_timer::outgoing::Stopped;
use k3k_timer::outgoing::TimerKind;
use k3k_timer::Timer;
use k3k_timer::TimerId;
use serial_test::serial;
use std::time::Duration;
use test_util::USER_1;
use test_util::USER_2;
use test_util::{common, TestContext};

/// Start a new time and clear ws queues
///
/// Returns the timer id
async fn start_timer(
    module_tester: &mut ModuleTester<Timer>,
    duration: Option<u64>,
    title: Option<String>,
    enable_ready_check: bool,
) -> TimerId {
    let start = incoming::Message::Start(incoming::Start {
        duration,
        title: title.clone(),
        enable_ready_check,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, start)
        .unwrap();

    let started1 = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    let (expected_duration, expected_kind) = match duration {
        Some(duration) => (duration, TimerKind::CountDown),
        None => (0, TimerKind::CountUp),
    };

    let timer_id =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(outgoing::Started {
            timer_id,
            kind: received_kind,
            duration: received_duration,
            title: received_title,
            ready_check_enabled: received_ready_check_enabled,
        })) = &started1
        {
            assert_eq!(received_kind, &expected_kind);

            assert_eq!(received_duration, &expected_duration);

            assert_eq!(received_title, &title);

            assert_eq!(received_ready_check_enabled, &enable_ready_check);

            *timer_id
        } else {
            panic!("Expected started message")
        };

    let started2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(started1, started2);

    timer_id
}

#[actix_rt::test]
#[serial]
async fn simple_stopwatch() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    start_timer(
        &mut module_tester,
        None,
        Some("This is a test".into()),
        false,
    )
    .await;
}

#[actix_rt::test]
#[serial]
async fn auto_stop() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    start_timer(
        &mut module_tester,
        Some(3),
        Some("This is a test".into()),
        false,
    )
    .await;

    // We should not have any messages in the ws que. Expect the receive_ws_message to timeout
    if let Ok(anything) = module_tester
        .receive_ws_message_override_timeout(&USER_1.participant_id, Duration::from_secs(0))
        .await
    {
        panic!("Did not expect Ws message, but received: {:?}", anything);
    }

    if let WsMessageOutgoing::Module(outgoing::Message::Stopped(Stopped {
        timer_id: _,
        kind,
        reason,
    })) = module_tester
        .receive_ws_message_override_timeout(&USER_1.participant_id, Duration::from_secs(5))
        .await
        .unwrap()
    {
        assert_eq!(kind, StopKind::Expired);
        assert_eq!(reason, None);
    } else {
        panic!("Expected to receive stop message at end of duration")
    }
}

#[actix_rt::test]
#[serial]
async fn manual_stop() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    let start_id = start_timer(
        &mut module_tester,
        None,
        Some("This is a test".into()),
        false,
    )
    .await;

    let stop = incoming::Message::Stop(Stop {
        timer_id: start_id,
        reason: Some("It is over".into()),
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, stop)
        .unwrap();

    let stopped1 = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Stopped(outgoing::Stopped {
        timer_id,
        kind,
        reason,
    })) = &stopped1
    {
        assert_eq!(*timer_id, start_id);

        assert_eq!(kind, &StopKind::ByModerator(USER_1.participant_id));

        assert_eq!(reason, &Some("It is over".into()));
    } else {
        panic!("Expected 'Stopped' message")
    }

    let stopped2 = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    assert_eq!(stopped1, stopped2)
}

#[actix_rt::test]
#[serial]
async fn ready_status() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    let start_id = start_timer(
        &mut module_tester,
        None,
        Some("This is a test".into()),
        true,
    )
    .await;

    let update_ready_status = incoming::Message::UpdateReadyStatus(incoming::UpdateReadyStatus {
        timer_id: start_id,
        status: true,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, update_ready_status)
        .unwrap();

    let update_ready_status = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::UpdatedReadyStatus(
        outgoing::UpdatedReadyStatus {
            timer_id,
            participant_id,
            status,
        },
    )) = update_ready_status
    {
        assert_eq!(timer_id, start_id);
        assert_eq!(participant_id, USER_1.participant_id);
        assert!(status);
    } else {
        panic!("Expected 'UpdatedReadyStatus' message")
    }
}

#[actix_rt::test]
#[serial]
async fn ready_status_toggle() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    let start_id = start_timer(
        &mut module_tester,
        None,
        Some("This is a test".into()),
        true,
    )
    .await;

    let update_ready_status = incoming::Message::UpdateReadyStatus(incoming::UpdateReadyStatus {
        timer_id: start_id,
        status: true,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, update_ready_status)
        .unwrap();

    let updated_ready_status = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::UpdatedReadyStatus(
        outgoing::UpdatedReadyStatus {
            timer_id,
            participant_id,
            status,
        },
    )) = updated_ready_status
    {
        assert_eq!(timer_id, start_id);
        assert_eq!(participant_id, USER_1.participant_id);
        assert!(status);
    } else {
        panic!("Expected 'UpdatedReadyStatus' message")
    }

    // update ready status to false

    let update_ready_status = incoming::Message::UpdateReadyStatus(incoming::UpdateReadyStatus {
        timer_id: start_id,
        status: false,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, update_ready_status)
        .unwrap();

    let update_ready_status = module_tester
        .receive_ws_message(&USER_2.participant_id)
        .await
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::UpdatedReadyStatus(
        outgoing::UpdatedReadyStatus {
            timer_id,
            participant_id,
            status,
        },
    )) = update_ready_status
    {
        assert_eq!(timer_id, start_id);
        assert_eq!(participant_id, USER_1.participant_id);
        assert!(!status);
    } else {
        panic!("Expected 'UpdatedReadyStatus' message")
    }
}

#[actix_rt::test]
#[serial]
async fn timer_already_active() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    start_timer(
        &mut module_tester,
        None,
        Some("This is a test".into()),
        true,
    )
    .await;

    let start = incoming::Message::Start(incoming::Start {
        duration: None,
        title: None,
        enable_ready_check: false,
    });

    module_tester
        .send_ws_message(&USER_1.participant_id, start)
        .unwrap();

    if let WsMessageOutgoing::Module(outgoing::Message::Error(
        outgoing::Error::TimerAlreadyRunning,
    )) = module_tester
        .receive_ws_message(&USER_1.participant_id)
        .await
        .unwrap()
    {
    } else {
        panic!("Expected 'TimerAlreadyRunning' error ");
    }
}
