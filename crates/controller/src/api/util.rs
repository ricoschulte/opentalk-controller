// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use phonenumber::PhoneNumber;

/// Try to parse a phone number and check its validity
///
/// Returns [`None`] the phone number is invalid or cannot be parsed
pub fn parse_phone_number(
    phone_number: &str,
    country_code: phonenumber::country::Id,
) -> Option<PhoneNumber> {
    // Remove characters from the phone number to make the parsing easier
    // user input may include any of these characters, but may not always be used correctly
    let phone_number = phone_number.replace(['(', ')', ' ', '-'], "");

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
