// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! This module contains types that are considered to be in the core of OpenTalk.
//!
//! All core types are simple types (e.g. newtypes of primitive or other simple types),
//! and typically used by other types in this crate.

mod asset_id;
mod group_name;
mod participant_id;
mod room_id;
mod tariff_id;
mod timestamp;

pub use asset_id::AssetId;
pub use group_name::GroupName;
pub use participant_id::ParticipantId;
pub use room_id::RoomId;
pub use tariff_id::TariffId;
pub use timestamp::Timestamp;
