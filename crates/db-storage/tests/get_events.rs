use chrono::{TimeZone as _, Utc};
use chrono_tz::Tz;
use database::DbConnection;
use k3k_db_storage::events::{
    Event, EventInvite, GetEventsCursor, NewEvent, NewEventInvite, TimeZone,
};
use k3k_db_storage::rooms::{NewRoom, RoomId};
use k3k_db_storage::users::{NewUser, NewUserWithGroups, UserId};
use serial_test::serial;

use crate::common::make_user;

mod common;

fn make_event(conn: &DbConnection, user_id: UserId, room_id: RoomId, hour: u32) -> Event {
    NewEvent {
        title: "Test Event".into(),
        description: "Test Event".into(),
        room: room_id,
        created_by: user_id,
        updated_by: user_id,
        is_time_independent: false,
        is_all_day: Some(false),
        starts_at: Some(Tz::UTC.ymd(2020, 1, 1).and_hms(hour, 0, 0)),
        starts_at_tz: Some(TimeZone(Tz::UTC)),
        ends_at: Some(Tz::UTC.ymd(2020, 1, 1).and_hms(hour, 0, 0)),
        ends_at_tz: Some(TimeZone(Tz::UTC)),
        duration_secs: Some(0),
        is_recurring: Some(false),
        recurrence_pattern: None,
    }
    .insert(conn)
    .unwrap()
}

#[tokio::test]
#[serial]
async fn test() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let conn = db_ctx.db.get_conn().unwrap();

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
    .insert(&conn)
    .unwrap();

    let room = NewRoom {
        created_by: user.id,
        password: None,
    }
    .insert(&conn)
    .unwrap();

    // create events. The variable number indicates its expected ordering

    // first two events, first on on hour 2 then 1.
    // This tests the ordering of comparison of times (e.g. starts_at, then created_at)
    let event2 = make_event(&conn, user.id, room.id, 2);
    let event1 = make_event(&conn, user.id, room.id, 1);

    // this event should come last because starts_at is largest
    let event8 = make_event(&conn, user.id, room.id, 10);

    // Test that created_at is being honored if starts_at is equal
    let event3 = make_event(&conn, user.id, room.id, 3);
    let event4 = make_event(&conn, user.id, room.id, 3);
    let event5 = make_event(&conn, user.id, room.id, 3);
    let event6 = make_event(&conn, user.id, room.id, 3);
    let event7 = make_event(&conn, user.id, room.id, 3);

    {
        // Test cursor

        // Get first two events 1, 2
        let first_two =
            Event::get_all_for_user_paginated(&conn, user.id, false, None, None, None, 2).unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event1 = &first_two[0].0;
        let query_event2 = &first_two[1].0;
        assert_eq!(query_event1, &event1);
        assert_eq!(query_event2, &event2);

        // Make cursor from last event fetched
        let cursor = GetEventsCursor::from_last_event_in_query(query_event2);

        // Use that to get 3,4
        let next_two =
            Event::get_all_for_user_paginated(&conn, user.id, false, None, None, Some(cursor), 2)
                .unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event3 = &next_two[0].0;
        let query_event4 = &next_two[1].0;
        assert_eq!(query_event3, &event3);
        assert_eq!(query_event4, &event4);

        // Then 5,6
        let cursor = GetEventsCursor::from_last_event_in_query(query_event4);

        let next_two =
            Event::get_all_for_user_paginated(&conn, user.id, false, None, None, Some(cursor), 2)
                .unwrap();
        assert_eq!(first_two.len(), 2);
        let query_event5 = &next_two[0].0;
        let query_event6 = &next_two[1].0;
        assert_eq!(query_event5, &event5);
        assert_eq!(query_event6, &event6);

        // Then 7,8
        let cursor = GetEventsCursor::from_last_event_in_query(query_event6);

        let next_two =
            Event::get_all_for_user_paginated(&conn, user.id, false, None, None, Some(cursor), 2)
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
            &conn,
            user.id,
            false,
            Some(Utc.ymd(2020, 1, 1).and_hms(5, 0, 0)),
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
            &conn,
            user.id,
            false,
            None,
            Some(Utc.ymd(2020, 1, 1).and_hms(5, 0, 0)),
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
            &conn,
            user.id,
            false,
            Some(Utc.ymd(2020, 1, 1).and_hms(3, 0, 0)),
            Some(Utc.ymd(2020, 1, 1).and_hms(3, 0, 0)),
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
async fn get_event_invites() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;

    let conn = db_ctx.db.get_conn().unwrap();

    let ferdinand = make_user(&conn, "Ferdinand", "Jaegermeister", "ferdemeister");
    let louise = make_user(&conn, "Jeez", "Louise", "Jesus");
    let gerhard = make_user(&conn, "Gerhard", "Bauer", "Hardi");

    let room = NewRoom {
        created_by: ferdinand.id,
        password: None,
    }
    .insert(&conn)
    .unwrap();

    // EVENT 1 MIT JEEZ LOUISE AND GERHARD
    let event1 = make_event(&conn, ferdinand.id, room.id, 1);

    NewEventInvite {
        event_id: event1.id,
        invitee: louise.id,
        created_by: ferdinand.id,
        created_at: None,
    }
    .insert(&conn)
    .unwrap();

    NewEventInvite {
        event_id: event1.id,
        invitee: gerhard.id,
        created_by: ferdinand.id,
        created_at: None,
    }
    .insert(&conn)
    .unwrap();

    // EVENT 2 MIT JEEZ LOUSE UND FERDINAND
    let event2 = make_event(&conn, gerhard.id, room.id, 1);

    NewEventInvite {
        event_id: event2.id,
        invitee: louise.id,
        created_by: gerhard.id,
        created_at: None,
    }
    .insert(&conn)
    .unwrap();

    NewEventInvite {
        event_id: event2.id,
        invitee: ferdinand.id,
        created_by: gerhard.id,
        created_at: None,
    }
    .insert(&conn)
    .unwrap();

    let events = &[&event1, &event2][..];
    let invites_with_invitees = EventInvite::get_for_events(&conn, events).unwrap();

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
