table! {
    use crate::sql_types::*;

    casbin_rule (id) {
        id -> Int4,
        ptype -> Varchar,
        v0 -> Varchar,
        v1 -> Varchar,
        v2 -> Varchar,
        v3 -> Varchar,
        v4 -> Varchar,
        v5 -> Varchar,
    }
}

table! {
    use crate::sql_types::*;

    event_exceptions (id) {
        id -> Uuid,
        event_id -> Uuid,
        exception_date -> Timestamptz,
        exception_date_tz -> Varchar,
        created_by -> Uuid,
        created_at -> Timestamptz,
        kind -> Event_exception_kind,
        title -> Nullable<Varchar>,
        description -> Nullable<Varchar>,
        is_all_day -> Nullable<Bool>,
        starts_at -> Nullable<Timestamptz>,
        starts_at_tz -> Nullable<Varchar>,
        ends_at -> Nullable<Timestamptz>,
        ends_at_tz -> Nullable<Varchar>,
    }
}

table! {
    use crate::sql_types::*;

    event_favorites (user_id, event_id) {
        user_id -> Uuid,
        event_id -> Uuid,
    }
}

table! {
    use crate::sql_types::*;

    event_invites (id) {
        id -> Uuid,
        event_id -> Uuid,
        invitee -> Uuid,
        created_by -> Uuid,
        created_at -> Timestamptz,
        status -> Event_invite_status,
    }
}

table! {
    use crate::sql_types::*;

    events (id) {
        id -> Uuid,
        id_serial -> Int8,
        title -> Varchar,
        description -> Varchar,
        room -> Uuid,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_by -> Uuid,
        updated_at -> Timestamptz,
        is_time_independent -> Bool,
        is_all_day -> Nullable<Bool>,
        starts_at -> Nullable<Timestamptz>,
        starts_at_tz -> Nullable<Varchar>,
        ends_at -> Nullable<Timestamptz>,
        ends_at_tz -> Nullable<Varchar>,
        duration_secs -> Nullable<Int4>,
        is_recurring -> Nullable<Bool>,
        recurrence_pattern -> Nullable<Varchar>,
    }
}

table! {
    use crate::sql_types::*;

    groups (id) {
        id -> Uuid,
        id_serial -> Int8,
        oidc_issuer -> Nullable<Text>,
        name -> Text,
    }
}

table! {
    use crate::sql_types::*;

    invites (id) {
        id -> Uuid,
        id_serial -> Int8,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_by -> Uuid,
        updated_at -> Timestamptz,
        room -> Uuid,
        active -> Bool,
        expiration -> Nullable<Timestamptz>,
    }
}

table! {
    use crate::sql_types::*;

    legal_votes (id) {
        id -> Uuid,
        id_serial -> Int8,
        created_by -> Uuid,
        created_at -> Timestamptz,
        room -> Nullable<Uuid>,
        protocol -> Jsonb,
    }
}

table! {
    use crate::sql_types::*;

    refinery_schema_history (version) {
        version -> Int4,
        name -> Nullable<Varchar>,
        applied_on -> Nullable<Varchar>,
        checksum -> Nullable<Varchar>,
    }
}

table! {
    use crate::sql_types::*;

    rooms (id) {
        id -> Uuid,
        id_serial -> Int8,
        created_by -> Uuid,
        created_at -> Timestamptz,
        password -> Varchar,
        wait_for_moderator -> Bool,
        listen_only -> Bool,
    }
}

table! {
    use crate::sql_types::*;

    sip_configs (id) {
        id -> Int8,
        room -> Uuid,
        sip_id -> Varchar,
        password -> Varchar,
        enable_lobby -> Bool,
    }
}

table! {
    use crate::sql_types::*;

    user_groups (user_id, group_id) {
        user_id -> Uuid,
        group_id -> Uuid,
    }
}

table! {
    use crate::sql_types::*;

    users (id) {
        id -> Uuid,
        id_serial -> Int8,
        oidc_sub -> Varchar,
        oidc_issuer -> Nullable<Text>,
        email -> Varchar,
        title -> Varchar,
        firstname -> Varchar,
        lastname -> Varchar,
        id_token_exp -> Int8,
        language -> Varchar,
        display_name -> Varchar,
        dashboard_theme -> Varchar,
        conference_theme -> Varchar,
    }
}

joinable!(event_exceptions -> events (event_id));
joinable!(event_exceptions -> users (created_by));
joinable!(event_favorites -> events (event_id));
joinable!(event_favorites -> users (user_id));
joinable!(event_invites -> events (event_id));
joinable!(events -> rooms (room));
joinable!(invites -> rooms (room));
joinable!(legal_votes -> rooms (room));
joinable!(legal_votes -> users (created_by));
joinable!(rooms -> users (created_by));
joinable!(sip_configs -> rooms (room));
joinable!(user_groups -> groups (group_id));
joinable!(user_groups -> users (user_id));

allow_tables_to_appear_in_same_query!(
    casbin_rule,
    event_exceptions,
    event_favorites,
    event_invites,
    events,
    groups,
    invites,
    legal_votes,
    refinery_schema_history,
    rooms,
    sip_configs,
    user_groups,
    users,
);
