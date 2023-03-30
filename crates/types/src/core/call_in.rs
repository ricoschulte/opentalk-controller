// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::imports::*;

crate::diesel_newtype! {
    NumericId(String) => diesel::sql_types::Text,
    CallInId(NumericId) => diesel::sql_types::Text,
    CallInPassword(NumericId) => diesel::sql_types::Text
}

impl NumericId {
    /// Generate a new random `NumericId`
    #[cfg(feature = "rand")]
    pub fn generate() -> Self {
        use rand::{distributions::Slice, thread_rng, Rng as _};

        /// The set of numbers used to generate [`SipId`] & [`SipPassword`]
        const NUMERIC: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
        let numeric_dist = Slice::new(&NUMERIC).unwrap();

        Self::from(thread_rng().sample_iter(numeric_dist).take(10).collect())
    }
}

#[cfg(feature = "serde")]
impl Validate for NumericId {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if self.inner().len() != 10 {
            errors.add("0", ValidationError::new("Invalid id length"));
            return Err(errors);
        }

        for c in self.inner().chars() {
            if !c.is_ascii_digit() {
                errors.add("0", ValidationError::new("Non numeric character"));
                return Err(errors);
            }
        }

        Ok(())
    }
}

impl CallInId {
    /// Generate a random sip id
    #[cfg(feature = "rand")]
    pub fn generate() -> Self {
        Self::from(NumericId::generate())
    }
}

#[cfg(feature = "serde")]
impl Validate for CallInId {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.inner().validate()
    }
}

impl CallInPassword {
    /// Generate a random sip password
    #[cfg(feature = "rand")]
    pub fn generate() -> Self {
        Self::from(NumericId::generate())
    }
}

#[cfg(feature = "serde")]
impl Validate for CallInPassword {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.inner().validate()
    }
}
