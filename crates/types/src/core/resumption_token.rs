// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::imports::*;
use derive_more::{Display, From, FromStr, Into};

/// A resumption token
#[derive(Display, From, FromStr, Into, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ResumptionToken(String);

impl ResumptionToken {
    /// Generate a new random resumption token
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
}
