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

joinable!(invites -> rooms (room));
joinable!(legal_votes -> rooms (room));
joinable!(legal_votes -> users (created_by));
joinable!(rooms -> users (created_by));
joinable!(sip_configs -> rooms (room));
joinable!(user_groups -> groups (group_id));
joinable!(user_groups -> users (user_id));

allow_tables_to_appear_in_same_query!(
    casbin_rule,
    groups,
    invites,
    legal_votes,
    refinery_schema_history,
    rooms,
    sip_configs,
    user_groups,
    users,
);
