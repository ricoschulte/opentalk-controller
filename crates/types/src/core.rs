// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! This module contains types that are considered to be in the core of OpenTalk.
//!
//! All core types are simple types (e.g. newtypes of primitive or other simple types),
//! and typically used by other types in this crate.

mod asset_id;
mod breakout_room_id;
mod date_time_tz;
mod event_id;
mod group_id;
mod group_name;
mod invite_code_id;
mod participant_id;
mod room_id;
mod tariff_id;
mod time_zone;
mod timestamp;

pub use asset_id::AssetId;
pub use breakout_room_id::BreakoutRoomId;
pub use date_time_tz::DateTimeTz;
pub use event_id::EventId;
pub use group_id::GroupId;
pub use group_name::GroupName;
pub use invite_code_id::InviteCodeId;
pub use participant_id::ParticipantId;
pub use room_id::RoomId;
pub use tariff_id::TariffId;
pub use time_zone::TimeZone;
pub use timestamp::Timestamp;
