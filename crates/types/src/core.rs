// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! This module contains types that are considered to be in the core of OpenTalk.
//!
//! All core types are simple types (e.g. newtypes of primitive or other simple types),
//! and typically used by other types in this crate.

mod asset_id;
mod participant_id;
mod room_id;
mod timestamp;

pub use asset_id::AssetId;
pub use participant_id::ParticipantId;
pub use room_id::RoomId;
pub use timestamp::Timestamp;
