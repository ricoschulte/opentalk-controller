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
        owner -> Int8,
        password -> Varchar,
        wait_for_moderator -> Bool,
        listen_only -> Bool,
    }
}

table! {
    users (id) {
        id -> Int8,
        oidc_uuid -> Uuid,
        email -> Varchar,
    }
}

joinable!(rooms -> users (owner));

allow_tables_to_appear_in_same_query!(
    refinery_schema_history,
    rooms,
    users,
);
