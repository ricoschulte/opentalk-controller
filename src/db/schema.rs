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

joinable!(rooms -> users (owner));

allow_tables_to_appear_in_same_query!(rooms, users,);
