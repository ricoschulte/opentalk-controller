// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! These modules includes migrations for changes to the used database.
//! Each module should have a function that returns the raw migration SQL string
//! and one function that returns the barrel migration.
pub mod v1;
