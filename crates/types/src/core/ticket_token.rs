// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::imports::*;
use derive_more::{Display, From, FromStr, Into};

/// A ticket token
#[derive(Display, From, FromStr, Into, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TicketToken(String);

impl TicketToken {
    /// Generate a random ticket token
    #[cfg(feature = "rand")]
    pub fn generate() -> Self {
        use rand::Rng;

        let token = rand::thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        Self(token)
    }

    /// Get a str reference to the data in the ticket token
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
