// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::LoginResult;
use crate::api::util::parse_phone_number;
use crate::oidc::IdTokenInfo;
use controller_shared::settings::Settings;
use database::DbConnection;
use db_storage::groups::{insert_user_into_groups, remove_user_from_groups, Group};
use db_storage::users::{UpdateUser, User};
use diesel::Connection;

/// Called when the `POST /auth/login` endpoint received an id-token with a `sub`+`tenant_id` combination that maps to
/// an existing user. Resets the expiry time of the id-token for the user. Also updates all fields in the database that
/// have changed since the last login.
///
/// The parameter `groups` contains the groups the user should be a part of, according to the id-token. This function
/// removes the user from all groups that are not in the list and adds them to the groups in the list.
///
/// Returns the user and all groups the user was removed from and added to.
pub(super) fn update_user(
    settings: &Settings,
    conn: &mut DbConnection,
    user: User,
    info: IdTokenInfo,
    groups: Vec<Group>,
) -> database::Result<LoginResult> {
    let changeset = create_changeset(settings, &user, &info);

    let user = changeset.apply(conn, user.id)?;

    conn.transaction(|conn| {
        let curr_groups = Group::get_all_for_user(conn, user.id)?;

        // Add user to added groups
        let groups_added_to = difference_by(&groups, &curr_groups, |group| &group.id);
        if !groups_added_to.is_empty() {
            insert_user_into_groups(conn, &user, &groups_added_to)?;
        }

        // Remove user from removed groups
        let groups_removed_from = difference_by(&curr_groups, &groups, |group| &group.id);
        if !groups_removed_from.is_empty() {
            remove_user_from_groups(conn, &user, &groups_removed_from)?;
        }

        Ok(LoginResult::UserUpdated {
            user,
            groups_added_to,
            groups_removed_from,
        })
    })
}

/// Create an [`UpdateUser`] changeset based on a comparison between `user` and `token_info`
fn create_changeset<'a>(
    settings: &Settings,
    user: &User,
    token_info: &'a IdTokenInfo,
) -> UpdateUser<'a> {
    let User {
        id: _,
        id_serial: _,
        oidc_sub: _,
        email,
        title: _,
        firstname,
        lastname,
        id_token_exp: _,
        language: _,
        display_name: _,
        dashboard_theme: _,
        conference_theme: _,
        phone,
        tenant_id: _,
    } = user;

    let mut changeset = UpdateUser {
        id_token_exp: Some(token_info.expiration.timestamp()),
        ..Default::default()
    };

    if firstname != &token_info.firstname {
        changeset.firstname = Some(&token_info.firstname);
    }

    if lastname != &token_info.lastname {
        changeset.lastname = Some(&token_info.lastname)
    }

    if email != &token_info.email {
        changeset.email = Some(&token_info.email);
    }

    let token_phone = if let Some((call_in, phone_number)) = settings
        .call_in
        .as_ref()
        .zip(token_info.phone_number.as_deref())
    {
        parse_phone_number(phone_number, call_in.default_country_code)
            .map(|p| p.format().mode(phonenumber::Mode::E164).to_string())
    } else {
        None
    };

    if phone != &token_phone {
        changeset.phone = Some(token_phone)
    }

    changeset
}

/// Returns all elements that are in `a` but no `b`
fn difference_by<T: Clone, C: PartialEq>(a: &[T], b: &[T], f: impl Fn(&T) -> &C) -> Vec<T> {
    a.iter()
        .filter(|a| !b.iter().any(|b| f(a) == f(b)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod test {
    use super::difference_by;

    #[test]
    fn difference() {
        let set_a = ['a', 'b', 'c'];
        let set_b = ['b', 'c', 'd'];

        let difference = difference_by(&set_a, &set_b, |c| c);

        assert_eq!(difference, ['a']);
    }
}
