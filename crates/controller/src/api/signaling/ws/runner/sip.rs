// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Utility to map a phone number to a users display name or convert it to a more readable format

use crate::api::util::parse_phone_number;
use crate::prelude::*;
use controller_shared::settings;
use database::Db;
use db_storage::users::{User, UserId};
use phonenumber::PhoneNumber;
use std::{convert::TryFrom, sync::Arc};

/// Try to map the provided phone number to a user
///
/// When the mapping fails, the phone number will be formatted to the international phone format.
///
/// Returns the display name for a given SIP display name, e.g. a phone number
pub async fn display_name(
    db: &Arc<Db>,
    settings: &settings::CallIn,
    created_by: UserId,
    phone_number: String,
) -> String {
    if let Some(parsed_number) = parse_phone_number(&phone_number, settings.default_country_code) {
        let display_name = try_map_to_user_display_name(db, created_by, &parsed_number).await;

        return display_name.unwrap_or_else(|| {
            parsed_number
                .format()
                .mode(phonenumber::Mode::International)
                .to_string()
        });
    }

    phone_number
}

/// Try to map the provided phone number to a user
///
/// The mapping fails if no user has the provided phone number configured or multiple
/// users have the provided phone number configured.
///
/// Returns [`None`] the phone number is invalid or cannot be parsed
async fn try_map_to_user_display_name(
    db: &Arc<Db>,
    created_by: UserId,
    phone_number: &PhoneNumber,
) -> Option<String> {
    let phone_e164 = phone_number
        .format()
        .mode(phonenumber::Mode::E164)
        .to_string();

    let db = db.clone();
    let result = crate::block(move || {
        let mut conn = db.get_conn()?;

        let creator = User::get(&mut conn, created_by)?;

        User::get_by_phone(&mut conn, &creator.oidc_issuer, &phone_e164)
    })
    .await;

    let users = match result {
        Ok(Ok(users)) => users,
        Ok(Err(err)) => {
            log::warn!(
                "Failed to get users by phone number from database {:?}",
                err
            );
            return None;
        }
        Err(err) => {
            log::error!("Blocking error in phone mapper: {:?}", err);
            return None;
        }
    };

    if let Ok([user]) = <[_; 1]>::try_from(users) {
        Some(user.display_name)
    } else {
        None
    }
}
