// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use controller::prelude::chrono::Duration;
use controller::prelude::chrono::Utc;
use controller::prelude::ModuleTester;
use controller::prelude::WsMessageOutgoing;
use k3k_timer::incoming;
use k3k_timer::incoming::Stop;
use k3k_timer::outgoing;
use k3k_timer::outgoing::StopKind;
use k3k_timer::outgoing::Stopped;
use k3k_timer::Timer;
use k3k_timer::TimerId;
use pretty_assertions::assert_eq;
use serial_test::serial;
use test_util::USER_1;
use test_util::USER_2;
use test_util::{common, TestContext};
use types::core::Timestamp;

/// Helps to compare expected timestamps.
#[derive(Debug)]
struct TimeFrame {
    start: Timestamp,
    end: Timestamp,
}

impl TimeFrame {
    /// Create a new [`TimeFrame`] from a timestamp and bounds
    ///
    /// The `ms_bound` parameter defines the upper and lower bound of the time frame.
    ///
    /// # Example
    ///
    /// A timer with `ms_bounds=50` will result in a 100 millisecond time frame and the
    /// provided `reference_timestamp` is the center of the frame
    fn new(reference_timestamp: &Timestamp, ms_bounds: i64) -> Self {
        Self {
            start: Timestamp::from(
                reference_timestamp
                    .checked_sub_signed(Duration::milliseconds(ms_bounds))
                    .expect("start timestamp overflow"),
            ),
            end: Timestamp::from(
                reference_timestamp
                    .checked_add_signed(Duration::milliseconds(ms_bounds))
                    .expect("end timestamp overflow"),
            ),
        }
    }

    fn now(ms_time_bounds: i64) -> Self {
        Self::new(&Timestamp::from(Utc::now()), ms_time_bounds)
    }

    /// Checks if the provided timestamp is contained in this [`TimeFrame`]
    fn contains(&self, value: &Timestamp) -> bool {
        value.ge(&self.start) && value.le(&self.end)
    }

    /// Creates a shifted copy
    fn shifted_by(&self, ms: i64) -> Self {
        Self {
            start: self
                .start
                .checked_add_signed(Duration::milliseconds(ms))
                .expect("start timestamp overflow while shifting")
                .into(),
            end: self
                .end
                .checked_add_signed(Duration::milliseconds(ms))
                .expect("start timestamp overflow while shifting")
                .into(),
        }
    }
}

/// Start a new time and clear ws queues
///
/// Returns the timer id
async fn start_timer(
    module_tester: &mut ModuleTester<Timer>,
    kind: incoming::Kind,
    style: Option<String>,
    title: Option<String>,
    enable_ready_check: bool,
) -> TimerId {
    // time frame to check if timestamps received by the timer module are within bounds
    //
    // The module tester does not do any networking when it comes to websocket and
    // rabbitmq messages. The timestamp offset from expected messages comes only
    // from cpu limitation.
    let time_frame = TimeFrame::now(50);

    let start = incoming::Message::Start(incoming::Start {
        kind,
        style: style.clone(),
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

    let timer_id =
        if let WsMessageOutgoing::Module(outgoing::Message::Started(outgoing::Started {
            timer_id,
            started_at,
            kind: received_kind,
            style: received_style,
            title: received_title,
            ready_check_enabled: received_ready_check_enabled,
        })) = &started1
        {
            assert!(time_frame.contains(started_at));

            if let outgoing::Kind::Countdown { ends_at } = received_kind {
                let configured_duration = match kind {
                    incoming::Kind::Countdown { duration } => duration,
                    incoming::Kind::Stopwatch => panic!("expected countdown kind"),
                };

                let time_shift_ms = (configured_duration * 1000) as i64;

                assert!(time_frame.shifted_by(time_shift_ms).contains(ends_at));
            }

            assert_eq!(received_style, &style);

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
        incoming::Kind::Stopwatch,
        None,
        Some("This is a test".into()),
        false,
    )
    .await;
}

#[actix_rt::test]
#[serial]
async fn coffee_break() {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    start_timer(
        &mut module_tester,
        incoming::Kind::Countdown { duration: 2 },
        Some("coffee_break".into()),
        None,
        true,
    )
    .await;
}

#[actix_rt::test]
#[serial]
async fn auto_stop_three_seconds() {
    simple_countdown(3).await
}

#[actix_rt::test]
#[serial]
async fn auto_stop_zero_seconds() {
    simple_countdown(0).await
}

async fn simple_countdown(duration: u64) {
    let test_ctx = TestContext::new().await;

    let (mut module_tester, _user1, _user2) = common::setup_users::<Timer>(&test_ctx, ()).await;

    start_timer(
        &mut module_tester,
        incoming::Kind::Countdown { duration },
        None,
        Some("This is a test".into()),
        false,
    )
    .await;

    // We should not have any messages in the ws que. Expect the receive_ws_message to timeout
    if let Ok(anything) = module_tester
        .receive_ws_message_override_timeout(
            &USER_1.participant_id,
            std::time::Duration::from_secs(0),
        )
        .await
    {
        panic!("Did not expect Ws message, but received: {anything:?}");
    }

    if let WsMessageOutgoing::Module(outgoing::Message::Stopped(Stopped {
        timer_id: _,
        kind,
        reason,
    })) = module_tester
        .receive_ws_message_override_timeout(
            &USER_1.participant_id,
            std::time::Duration::from_secs(5),
        )
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
        incoming::Kind::Stopwatch,
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
        incoming::Kind::Stopwatch,
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
        incoming::Kind::Stopwatch,
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
        incoming::Kind::Stopwatch,
        None,
        Some("This is a test".into()),
        true,
    )
    .await;

    let start = incoming::Message::Start(incoming::Start {
        kind: incoming::Kind::Stopwatch,
        style: None,
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
