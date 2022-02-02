table! {
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
    groups (id) {
        id -> Uuid,
        id_serial -> Int8,
        oidc_issuer -> Nullable<Text>,
        name -> Text,
    }
}

table! {
    invites (id) {
        id_serial -> Int8,
        id -> Uuid,
        created -> Timestamptz,
        updated -> Timestamptz,
        room -> Uuid,
        active -> Bool,
        expiration -> Nullable<Timestamptz>,
        created_by -> Uuid,
        updated_by -> Uuid,
    }
}

table! {
    legal_votes (id) {
        id -> Uuid,
        protocol -> Jsonb,
        room_id -> Nullable<Uuid>,
        id_serial -> Int8,
        initiator -> Uuid,
    }
}

table! {
    refinery_schema_history (version) {
        version -> Int4,
        name -> Nullable<Varchar>,
        applied_on -> Nullable<Varchar>,
        checksum -> Nullable<Varchar>,
    }
}

table! {
    rooms (id) {
        id_serial -> Int8,
        id -> Uuid,
        password -> Varchar,
        wait_for_moderator -> Bool,
        listen_only -> Bool,
        owner -> Uuid,
    }
}

table! {
    sip_configs (id) {
        id -> Int8,
        room -> Uuid,
        sip_id -> Varchar,
        password -> Varchar,
        enable_lobby -> Bool,
    }
}

table! {
    user_groups (user_id, group_id) {
        user_id -> Uuid,
        group_id -> Uuid,
    }
}

table! {
    users (id) {
        id_serial -> Int8,
        oidc_sub -> Varchar,
        email -> Varchar,
        title -> Varchar,
        firstname -> Varchar,
        lastname -> Varchar,
        id_token_exp -> Int8,
        theme -> Varchar,
        language -> Varchar,
        id -> Uuid,
        oidc_issuer -> Nullable<Text>,
    }
}

joinable!(invites -> rooms (room));
joinable!(legal_votes -> rooms (room_id));
joinable!(legal_votes -> users (initiator));
joinable!(rooms -> users (owner));
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
