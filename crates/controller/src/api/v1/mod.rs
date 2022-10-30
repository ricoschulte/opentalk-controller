// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! REST API v1
//!
//! Current Endpoints. See their respective function:
//! - `/auth/login` ([post](auth::login))
//! - `/rooms` ([GET](rooms::accessible), [POST](rooms::new))
//! - `/rooms/{room_id}` ([GET](rooms::get), [PATCH](rooms::patch))
//! - `/rooms/{room_id}/start` ([POST](rooms::start))
//! - `/rooms/{room_id}/start_invited` ([POST](rooms::start_invited))
//! - `/rooms/{room_id}/invites ([GET](invites::get_invites), [POST](invites::add_invite))
//! - `/rooms/{room_id}/invites/{invite_code} ([GET](invites::get_invite), [PUT](invites::update_invite), [DELETE](invites::delete_invite)])
//! - `/rooms/{room_id}/sip ([GET](sip_configs::get), [PUT](sip_configs::put), [DELETE](sip_configs::delete))
//! - `/rooms/{room_id}/legal_votes ([GET](legal_vote::get_all_for_room))
//! - `/turn` ([GET](turn::get))
//! - `/users/me`([GET](users::get_me), [PATCH](users::patch_me))
//! - `/users/{user_id}` ([GET](users::get_user))
//! - `/users/find` ([GET](users::find))
//! - `/legal_votes` ([GET](legal_vote::get_all))
//! - `/legal_votes/{legal_vote_id}` ([GET](legal_vote::get_specific))
//! - `/services/call_in/start ([POST](services::call_in::start))

pub use request::{CursorPaginationQuery, PagePaginationQuery};
pub use response::{ApiResponse, DefaultApiResult};

pub mod assets;
pub mod auth;
mod cursor;
pub mod events;
pub mod invites;
pub mod legal_vote;
pub mod middleware;
mod request;
pub mod response;
pub mod rooms;
pub mod services;
pub mod sip_configs;
pub mod turn;
pub mod users;
mod util;
