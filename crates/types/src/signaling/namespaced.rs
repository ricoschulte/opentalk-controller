// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{core::Timestamp, imports::*};

/// An envelope of a command annotated with their respective module name.
///
/// This is used for WebSocket messages sent to the backend.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct NamespacedCommand<'n, O> {
    /// The namespace to which the message is targeted
    pub namespace: &'n str,
    /// The payload of the message
    pub payload: O,
}

/// An envelope of an event annotated with their respective module name.
///
/// This is used for WebSocket messages sent to the frontend.
/// Similar to [`NamespacedCommand`], but includes a timestamp field.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct NamespacedEvent<'n, O> {
    /// The namespace to which the message is targeted
    pub namespace: &'n str,
    /// The timestamp indicating the creation of the message
    pub timestamp: Timestamp,
    /// The payload of the message
    pub payload: O,
}
