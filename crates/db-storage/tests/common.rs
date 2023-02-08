// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use database::DbConnection;
use k3k_db_storage::users::{NewUser, NewUserWithGroups, User};

pub fn make_user(
    conn: &mut DbConnection,
    firstname: &str,
    lastname: &str,
    display_name: &str,
) -> User {
    NewUserWithGroups {
        new_user: NewUser {
            email: format!(
                "{}.{}@example.org",
                firstname.to_lowercase(),
                lastname.to_lowercase()
            ),
            title: "".into(),
            firstname: firstname.into(),
            lastname: lastname.into(),
            id_token_exp: 0,
            display_name: display_name.into(),
            language: "".into(),
            oidc_sub: format!("{firstname}{lastname}"),
            phone: None,
        },
        groups: vec![],
    }
    .insert(conn)
    .unwrap()
    .0
}
