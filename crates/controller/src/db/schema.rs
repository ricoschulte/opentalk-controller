table! {
    groups (id) {
        id -> Varchar,
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

joinable!(legal_votes -> users (initiator));
joinable!(rooms -> users (owner));
joinable!(user_groups -> groups (group_id));
joinable!(user_groups -> users (user_id));

allow_tables_to_appear_in_same_query!(
    groups,
    legal_votes,
    refinery_schema_history,
    rooms,
    user_groups,
    users,
);
