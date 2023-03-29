// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

crate::diesel_newtype! {
    #[derive(Copy)]
    TariffId(uuid::Uuid) => diesel::sql_types::Uuid
}

impl TariffId {
    /// Create a ZERO tariff id, e.g. for testing purposes
    pub const fn nil() -> Self {
        Self::from(uuid::Uuid::nil())
    }
}
