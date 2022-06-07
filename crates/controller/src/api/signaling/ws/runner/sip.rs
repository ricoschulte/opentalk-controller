//! Utility to map a phone number to a users display name or convert it to a more readable format

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
        let conn = db.get_conn()?;

        let creator = User::get(&conn, created_by)?;

        User::get_by_phone(&conn, &creator.oidc_issuer, &phone_e164)
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

/// Try to parse a phone number and check its validity
///
/// Returns [`None`] the phone number is invalid or cannot be parsed
fn parse_phone_number(
    phone_number: &str,
    country_code: phonenumber::country::Id,
) -> Option<PhoneNumber> {
    // Catch panics because the phonenumber crate has some questionable unwraps
    let result =
        std::panic::catch_unwind(move || phonenumber::parse(Some(country_code), phone_number));

    // check if phonenumber crate panicked or failed to parse
    let phone_number = match result {
        Ok(Ok(phone)) => phone,
        Ok(Err(err)) => {
            log::warn!("failed to parse phone number: {:?}", err);
            return None;
        }
        Err(err) => {
            log::error!(
                "phonenumber crate panicked while parsing phone number: {:?}",
                err
            );
            return None;
        }
    };

    if !phonenumber::is_valid(&phone_number) {
        return None;
    }

    Some(phone_number)
}
