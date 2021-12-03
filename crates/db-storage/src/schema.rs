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
        id -> Varchar,
    }
}

table! {
    invites (id) {
        id -> Int8,
        uuid -> Uuid,
        created -> Timestamptz,
        created_by -> Int8,
        updated -> Timestamptz,
        updated_by -> Int8,
        room -> Uuid,
        active -> Bool,
        expiration -> Nullable<Timestamptz>,
    }
}

table! {
    legal_vote_room (vote_id) {
        vote_id -> Uuid,
        room_id -> Uuid,
    }
}

table! {
    legal_votes (id) {
        id -> Uuid,
        initiator -> Int8,
        protocol -> Jsonb,
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
        id -> Int8,
        uuid -> Uuid,
        owner -> Int8,
        password -> Varchar,
        wait_for_moderator -> Bool,
        listen_only -> Bool,
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
        user_id -> Int8,
        group_id -> Varchar,
    }
}

table! {
    users (id) {
        id -> Int8,
        oidc_uuid -> Uuid,
        email -> Varchar,
        title -> Varchar,
        firstname -> Varchar,
        lastname -> Varchar,
        id_token_exp -> Int8,
        theme -> Varchar,
        language -> Varchar,
    }
}

joinable!(legal_vote_room -> legal_votes (vote_id));
joinable!(legal_votes -> users (initiator));
joinable!(rooms -> users (owner));
joinable!(user_groups -> groups (group_id));
joinable!(user_groups -> users (user_id));

allow_tables_to_appear_in_same_query!(
    casbin_rule,
    groups,
    invites,
    legal_vote_room,
    legal_votes,
    refinery_schema_history,
    rooms,
    sip_configs,
    user_groups,
    users,
);
