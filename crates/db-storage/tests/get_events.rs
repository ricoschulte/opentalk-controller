// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use chrono::{TimeZone as _, Utc};
use chrono_tz::Tz;
use database::DbConnection;
use k3k_db_storage::events::{
    Event, EventId, EventInvite, EventInviteStatus, GetEventsCursor, NewEvent, NewEventInvite,
    TimeZone, UpdateEventInvite,
};
use k3k_db_storage::rooms::{NewRoom, RoomId};
use k3k_db_storage::users::{NewUser, NewUserWithGroups, UserId};
use pretty_assertions::assert_eq;
use serial_test::serial;

use crate::common::make_user;

mod common;

fn make_event(
    conn: &mut DbConnection,
    user_id: UserId,
    room_id: RoomId,
    hour: Option<u32>,
    is_adhoc: bool,
) -> Event {
    NewEvent {
        title: "Test Event".into(),
        description: "Test Event".into(),
        room: room_id,
        created_by: user_id,
        updated_by: user_id,
        is_time_independent: hour.is_none(),
        is_all_day: Some(false),
        starts_at: hour.map(|h| Tz::UTC.ymd(2020, 1, 1).and_hms(h, 0, 0)),
        starts_at_tz: hour.map(|_| TimeZone(Tz::UTC)),
        ends_at: hour.map(|h| Tz::UTC.ymd(2020, 1, 1).and_hms(h, 0, 0)),
        ends_at_tz: hour.map(|_| TimeZone(Tz::UTC)),
        duration_secs: hour.map(|_| 0),
        is_recurring: Some(false),
        recurrence_pattern: None,
        is_adhoc,
    }
    .insert(conn)
    .unwrap()
}

fn update_invite_status(
    conn: &mut DbConnection,
    user_id: UserId,
    event_id: EventId,
    new_status: EventInviteStatus,
) {
    let changeset = UpdateEventInvite { status: new_status };

    changeset.apply(conn, user_id, event_id).unwrap();
}

#[tokio::test]
#[serial]
async fn test() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut conn = db_ctx.db.get_conn().unwrap();

    let (user, _) = NewUserWithGroups {
        new_user: NewUser {
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Tester".into(),
            id_token_exp: 0,
            language: "".into(),
            display_name: "Test Tester".into(),
            oidc_sub: "testtestersoidcsub".into(),
            oidc_issuer: "".into(),
            phone: None,
        },
        groups: vec![],
    }
    .insert(&mut conn)
    .unwrap();

    let room = NewRoom {
        created_by: user.id,
        password: None,
        waiting_room: false,
    }
    .insert(&mut conn)
    .unwrap();

    // create events. The variable number indicates its expected ordering

    // first two events, first on on hour 2 then 1.
    // This tests the ordering of comparison of times (e.g. starts_at, then created_at)
    let event2 = make_event(&mut conn, user.id, room.id, Some(2), true);
    let event1 = make_event(&mut conn, user.id, room.id, Some(1), true);

    // this event should come last because starts_at is largest
    let event8 = make_event(&mut conn, user.id, room.id, Some(10), true);

    // Test that created_at is being honored if starts_at is equal
    let event3 = make_event(&mut conn, user.id, room.id, Some(3), true);
    let event4 = make_event(&mut conn, user.id, room.id, Some(3), true);
    let event5 = make_event(&mut conn, user.id, room.id, Some(3), true);
    let event6 = make_event(&mut conn, user.id, room.id, Some(3), true);
    let event7 = make_event(&mut conn, user.id, room.id, Some(3), true);

    {
        // Test cursor

        // Get first two events 1, 2
        let first_two = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            None,
            None,
            None,
            None,
            2,
        )
        .unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event1 = &first_two[0].0;
        let query_event2 = &first_two[1].0;
        assert_eq!(query_event1, &event1);
        assert_eq!(query_event2, &event2);

        // Make cursor from last event fetched
        let cursor = GetEventsCursor::from_last_event_in_query(query_event2);

        // Use that to get 3,4
        let next_two = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            None,
            None,
            None,
            Some(cursor),
            2,
        )
        .unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event3 = &next_two[0].0;
        let query_event4 = &next_two[1].0;
        assert_eq!(query_event3, &event3);
        assert_eq!(query_event4, &event4);

        // Then 5,6
        let cursor = GetEventsCursor::from_last_event_in_query(query_event4);

        let next_two = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            None,
            None,
            None,
            Some(cursor),
            2,
        )
        .unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event5 = &next_two[0].0;
        let query_event6 = &next_two[1].0;
        assert_eq!(query_event5, &event5);
        assert_eq!(query_event6, &event6);

        // Then 7,8
        let cursor = GetEventsCursor::from_last_event_in_query(query_event6);

        let next_two = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            None,
            None,
            None,
            Some(cursor),
            2,
        )
        .unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event7 = &next_two[0].0;
        let query_event8 = &next_two[1].0;
        assert_eq!(query_event7, &event7);
        assert_eq!(query_event8, &event8);
    }

    {
        // Test time_min
        let only_event8 = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(5, 0, 0)),
            None,
            None,
            None,
            None,
            100,
        )
        .unwrap();
        assert_eq!(only_event8.len(), 1);
        assert_eq!(only_event8[0].0, event8);
    }

    {
        // Test time_max
        let every_event_except_event8 = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            Some(Utc.ymd(2020, 1, 1).and_hms(5, 0, 0)),
            None,
            None,
            None,
            100,
        )
        .unwrap();
        assert_eq!(every_event_except_event8.len(), 7);
        assert_eq!(every_event_except_event8[0].0, event1);
        assert_eq!(every_event_except_event8[1].0, event2);
        assert_eq!(every_event_except_event8[2].0, event3);
        assert_eq!(every_event_except_event8[3].0, event4);
        assert_eq!(every_event_except_event8[4].0, event5);
        assert_eq!(every_event_except_event8[5].0, event6);
        assert_eq!(every_event_except_event8[6].0, event7);
        assert_eq!(every_event_except_event8[0].0, event1);
    }
    {
        // Test both time_min + time_max
        let only_event_at_3h = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(3, 0, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(3, 0, 0)),
            None,
            None,
            None,
            100,
        )
        .unwrap();
        assert_eq!(only_event_at_3h.len(), 5);
        assert_eq!(only_event_at_3h[0].0, event3);
        assert_eq!(only_event_at_3h[1].0, event4);
        assert_eq!(only_event_at_3h[2].0, event5);
        assert_eq!(only_event_at_3h[3].0, event6);
        assert_eq!(only_event_at_3h[4].0, event7);
    }
}

#[tokio::test]
#[serial]
async fn get_events_invite_filter() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut conn = db_ctx.db.get_conn().unwrap();

    let inviter = make_user(&mut conn, "Ingo", "Inviter", "inviter");
    let invitee = make_user(&mut conn, "Ingrid", "Invitee", "invitee");

    let room = NewRoom {
        created_by: inviter.id,
        password: None,
        waiting_room: false,
    }
    .insert(&mut conn)
    .unwrap();

    let accept_event = make_event(&mut conn, inviter.id, room.id, Some(1), true);
    let decline_event = make_event(&mut conn, inviter.id, room.id, Some(1), true);
    let tentative_event = make_event(&mut conn, inviter.id, room.id, Some(1), true);
    let pending_event = make_event(&mut conn, inviter.id, room.id, Some(1), true);

    // Check that the creator of the events gets created events when filtering by `Accepted` invite status
    let all_events = Event::get_all_for_user_paginated(
        &mut conn,
        inviter.id,
        false,
        vec![EventInviteStatus::Accepted],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert_eq!(all_events.len(), 4);
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == accept_event.id));
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == decline_event.id));
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == tentative_event.id));
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == pending_event.id));

    // Check that no events are returned when filtering for `Declined`
    let no_events = Event::get_all_for_user_paginated(
        &mut conn,
        inviter.id,
        false,
        vec![EventInviteStatus::Declined],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert!(no_events.is_empty());

    let events = vec![
        &accept_event,
        &decline_event,
        &tentative_event,
        &pending_event,
    ];

    // invite the invitee to all events
    for event in events {
        NewEventInvite {
            event_id: event.id,
            invitee: invitee.id,
            created_by: inviter.id,
            created_at: None,
        }
        .try_insert(&mut conn)
        .unwrap();
    }

    update_invite_status(
        &mut conn,
        invitee.id,
        accept_event.id,
        EventInviteStatus::Accepted,
    );

    update_invite_status(
        &mut conn,
        invitee.id,
        decline_event.id,
        EventInviteStatus::Declined,
    );

    update_invite_status(
        &mut conn,
        invitee.id,
        tentative_event.id,
        EventInviteStatus::Tentative,
    );

    // check `accepted` invites
    let accepted_events = Event::get_all_for_user_paginated(
        &mut conn,
        invitee.id,
        false,
        vec![EventInviteStatus::Accepted],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert_eq!(accepted_events.len(), 1);
    assert!(accepted_events
        .iter()
        .any(|(event, ..)| event.id == accept_event.id));

    // check `declined` invites
    let declined_events = Event::get_all_for_user_paginated(
        &mut conn,
        invitee.id,
        false,
        vec![EventInviteStatus::Declined],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert_eq!(declined_events.len(), 1);
    assert!(declined_events
        .iter()
        .any(|(event, ..)| event.id == decline_event.id));

    // check `tentative` invites
    let tentative_events = Event::get_all_for_user_paginated(
        &mut conn,
        invitee.id,
        false,
        vec![EventInviteStatus::Tentative],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert_eq!(tentative_events.len(), 1);
    assert!(tentative_events
        .iter()
        .any(|(event, ..)| event.id == tentative_event.id));

    // check `pending` invites
    let pending_events = Event::get_all_for_user_paginated(
        &mut conn,
        invitee.id,
        false,
        vec![EventInviteStatus::Pending],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert_eq!(pending_events.len(), 1);
    assert!(pending_events
        .iter()
        .any(|(event, ..)| event.id == pending_event.id));

    // expect all events when no invite_status_filter is set
    let all_events = Event::get_all_for_user_paginated(
        &mut conn,
        invitee.id,
        false,
        vec![],
        None,
        None,
        None,
        None,
        None,
        100,
    )
    .unwrap();

    assert_eq!(all_events.len(), 4);
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == accept_event.id));
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == decline_event.id));
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == tentative_event.id));
    assert!(all_events
        .iter()
        .any(|(event, ..)| event.id == pending_event.id));
}

#[tokio::test]
#[serial]
async fn get_event_invites() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut conn = db_ctx.db.get_conn().unwrap();

    let ferdinand = make_user(&mut conn, "Ferdinand", "Jaegermeister", "ferdemeister");
    let louise = make_user(&mut conn, "Jeez", "Louise", "Jesus");
    let gerhard = make_user(&mut conn, "Gerhard", "Bauer", "Hardi");

    let room = NewRoom {
        created_by: ferdinand.id,
        password: None,
        waiting_room: false,
    }
    .insert(&mut conn)
    .unwrap();

    // EVENT 1 MIT JEEZ LOUISE AND GERHARD
    let event1 = make_event(&mut conn, ferdinand.id, room.id, Some(1), true);

    NewEventInvite {
        event_id: event1.id,
        invitee: louise.id,
        created_by: ferdinand.id,
        created_at: None,
    }
    .try_insert(&mut conn)
    .unwrap();

    NewEventInvite {
        event_id: event1.id,
        invitee: gerhard.id,
        created_by: ferdinand.id,
        created_at: None,
    }
    .try_insert(&mut conn)
    .unwrap();

    // EVENT 2 MIT JEEZ LOUSE UND FERDINAND
    let event2 = make_event(&mut conn, gerhard.id, room.id, Some(1), true);

    NewEventInvite {
        event_id: event2.id,
        invitee: louise.id,
        created_by: gerhard.id,
        created_at: None,
    }
    .try_insert(&mut conn)
    .unwrap();

    NewEventInvite {
        event_id: event2.id,
        invitee: ferdinand.id,
        created_by: gerhard.id,
        created_at: None,
    }
    .try_insert(&mut conn)
    .unwrap();

    let events = &[&event1, &event2][..];
    let invites_with_invitees = EventInvite::get_for_events(&mut conn, events).unwrap();

    for (event, invites_with_users) in events.iter().zip(invites_with_invitees) {
        println!("Event: {:#?}", event);
        println!(
            "Invitees: {:#?}",
            invites_with_users
                .into_iter()
                .map(|x| x.1)
                .collect::<Vec<_>>()
        );
        println!("#################################################")
    }
}

#[tokio::test]
#[serial]
async fn get_event_adhoc() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut conn = db_ctx.db.get_conn().unwrap();

    let (user, _) = NewUserWithGroups {
        new_user: NewUser {
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Tester".into(),
            id_token_exp: 0,
            language: "".into(),
            display_name: "Test Tester".into(),
            oidc_sub: "testtestersoidcsub".into(),
            oidc_issuer: "".into(),
            phone: None,
        },
        groups: vec![],
    }
    .insert(&mut conn)
    .unwrap();

    let room = NewRoom {
        created_by: user.id,
        password: None,
        waiting_room: false,
    }
    .insert(&mut conn)
    .unwrap();

    let event1 = make_event(&mut conn, user.id, room.id, Some(1), true);
    let event2 = make_event(&mut conn, user.id, room.id, Some(1), false);
    let event3 = make_event(&mut conn, user.id, room.id, Some(1), false);
    let event4 = make_event(&mut conn, user.id, room.id, Some(1), true);
    let event5 = make_event(&mut conn, user.id, room.id, Some(1), true);

    let all = Event::get_all_for_user_paginated(
        &mut conn,
        user.id,
        false,
        vec![],
        None,
        None,
        None,
        None,
        None,
        10,
    )
    .unwrap();

    assert_eq!(all.len(), 5);
    assert_eq!(all[0].0, event1);
    assert_eq!(all[1].0, event2);
    assert_eq!(all[2].0, event3);
    assert_eq!(all[3].0, event4);
    assert_eq!(all[4].0, event5);

    let adhoc = Event::get_all_for_user_paginated(
        &mut conn,
        user.id,
        false,
        vec![],
        None,
        None,
        Some(true),
        None,
        None,
        10,
    )
    .unwrap();
    assert_eq!(adhoc.len(), 3);
    assert_eq!(adhoc[0].0, event1);
    assert_eq!(adhoc[1].0, event4);
    assert_eq!(adhoc[2].0, event5);

    let non_adhoc = Event::get_all_for_user_paginated(
        &mut conn,
        user.id,
        false,
        vec![],
        None,
        None,
        Some(false),
        None,
        None,
        10,
    )
    .unwrap();
    assert_eq!(non_adhoc.len(), 2);
    assert_eq!(non_adhoc[0].0, event2);
    assert_eq!(non_adhoc[1].0, event3);
}

#[tokio::test]
#[serial]
async fn get_event_time_independent() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut conn = db_ctx.db.get_conn().unwrap();

    let (user, _) = NewUserWithGroups {
        new_user: NewUser {
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Tester".into(),
            id_token_exp: 0,
            language: "".into(),
            display_name: "Test Tester".into(),
            oidc_sub: "testtestersoidcsub".into(),
            oidc_issuer: "".into(),
            phone: None,
        },
        groups: vec![],
    }
    .insert(&mut conn)
    .unwrap();

    let room = NewRoom {
        created_by: user.id,
        password: None,
        waiting_room: false,
    }
    .insert(&mut conn)
    .unwrap();

    let event1 = make_event(&mut conn, user.id, room.id, None, false);
    let event2 = make_event(&mut conn, user.id, room.id, None, false);
    let event3 = make_event(&mut conn, user.id, room.id, Some(1), false);
    let event4 = make_event(&mut conn, user.id, room.id, None, false);
    let event5 = make_event(&mut conn, user.id, room.id, Some(2), false);

    let all = Event::get_all_for_user_paginated(
        &mut conn,
        user.id,
        false,
        vec![],
        None,
        None,
        None,
        None,
        None,
        10,
    )
    .unwrap();

    // different order than creation order, because events without
    // starts_at field are sorted first
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].0, event1);
    assert_eq!(all[1].0, event2);
    assert_eq!(all[2].0, event4);
    assert_eq!(all[3].0, event3);
    assert_eq!(all[4].0, event5);

    let time_independent = Event::get_all_for_user_paginated(
        &mut conn,
        user.id,
        false,
        vec![],
        None,
        None,
        None,
        Some(true),
        None,
        10,
    )
    .unwrap();
    assert_eq!(time_independent.len(), 3);
    assert_eq!(time_independent[0].0, event1);
    assert_eq!(time_independent[1].0, event2);
    assert_eq!(time_independent[2].0, event4);

    let time_dependent = Event::get_all_for_user_paginated(
        &mut conn,
        user.id,
        false,
        vec![],
        None,
        None,
        None,
        Some(false),
        None,
        10,
    )
    .unwrap();
    assert_eq!(time_dependent.len(), 2);
    assert_eq!(time_dependent[0].0, event3);
    assert_eq!(time_dependent[1].0, event5);
}

#[tokio::test]
#[serial]
async fn get_event_min_max_time() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut conn = db_ctx.db.get_conn().unwrap();

    let (user, _) = NewUserWithGroups {
        new_user: NewUser {
            email: "test@example.org".into(),
            title: "".into(),
            firstname: "Test".into(),
            lastname: "Tester".into(),
            id_token_exp: 0,
            language: "".into(),
            display_name: "Test Tester".into(),
            oidc_sub: "testtestersoidcsub".into(),
            oidc_issuer: "".into(),
            phone: None,
        },
        groups: vec![],
    }
    .insert(&mut conn)
    .unwrap();

    let room = NewRoom {
        created_by: user.id,
        password: None,
        waiting_room: false,
    }
    .insert(&mut conn)
    .unwrap();

    let event1 = NewEvent {
        title: "Test Event".into(),
        description: "Test Event".into(),
        room: room.id,
        created_by: user.id,
        updated_by: user.id,
        is_time_independent: false,
        is_all_day: Some(false),
        starts_at: None,
        starts_at_tz: None,
        ends_at: None,
        ends_at_tz: None,
        duration_secs: None,
        is_recurring: Some(false),
        recurrence_pattern: None,
        is_adhoc: false,
    }
    .insert(&mut conn)
    .unwrap();

    let event2 = NewEvent {
        title: "Test Event".into(),
        description: "Test Event".into(),
        room: room.id,
        created_by: user.id,
        updated_by: user.id,
        is_time_independent: false,
        is_all_day: Some(false),
        starts_at: Some(Tz::UTC.ymd(2020, 1, 1).and_hms(10, 0, 0)),
        starts_at_tz: Some(TimeZone(Tz::UTC)),
        ends_at: Some(Tz::UTC.ymd(2020, 1, 1).and_hms(11, 0, 0)),
        ends_at_tz: Some(TimeZone(Tz::UTC)),
        duration_secs: Some(3600),
        is_recurring: Some(false),
        recurrence_pattern: None,
        is_adhoc: false,
    }
    .insert(&mut conn)
    .unwrap();

    {
        // Query without any time restrictions
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, event1);
        assert_eq!(events[1].0, event2);
    }

    {
        // Query an open timeframe before the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            Some(Utc.ymd(2020, 1, 1).and_hms(9, 0, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert!(events.is_empty());
    }

    {
        // Query a closed timeframe before the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(8, 0, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(9, 0, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert!(events.is_empty());
    }

    {
        // Query an open timeframe after the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(12, 0, 0)),
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert!(events.is_empty());
    }

    {
        // Query an closed timeframe after the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(12, 0, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(13, 0, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert!(events.is_empty());
    }

    {
        // Query a timeframe ending at the start of the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 0, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query a timeframe starting at the end of the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(11, 0, 0)),
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query an open timeframe overlapping the first half of the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            None,
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 30, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query a timeframe overlapping the first half of the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(9, 30, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 30, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query an open timeframe overlapping the second half of the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 30, 0)),
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query a timeframe overlapping the second half of the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 30, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(11, 30, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query a timeframe fully inside the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 20, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(10, 40, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }

    {
        // Query a timeframe surrounding the event
        let events = Event::get_all_for_user_paginated(
            &mut conn,
            user.id,
            false,
            vec![],
            Some(Utc.ymd(2020, 1, 1).and_hms(9, 0, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(12, 0, 0)),
            None,
            None,
            None,
            10,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, event2);
    }
}
