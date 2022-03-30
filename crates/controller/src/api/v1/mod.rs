//! REST API v1
//!
//! Current Endpoints. See their respective function:
//! - `/auth/login` ([post](auth::login))
//! - `/rooms` ([GET](rooms::owned), [POST](rooms::new))
//! - `/rooms/{room_id}` ([GET](rooms::get), [PUT](rooms::modify))
//! - `/rooms/{room_id}/start` ([POST](rooms::start))
//! - `/rooms/{room_id}/start_invited` ([POST](rooms::start_invited))
//! - `/rooms/{room_id}/invites ([GET](invites::get_invites), [POST](invites::add_invite))
//! - `/rooms/{room_id}/invites/{invite_code} ([GET](invites::get_invite), [PUT](invites::update_invite), [DELETE](invites::delete_invite)])
//! - `/rooms/sip/start ([POST](rooms::sip_start))
//! - `/rooms/{room_id}/sip ([GET](sip_configs::get), [PUT](sip_configs::put), [DELETE](sip_configs::delete))
//! - `/rooms/{room_id}/legal_votes ([GET](legal_vote::get_all_for_room))
//! - `/turn` ([GET](turn::get))
//! - `/users` ([GET](users::all))
//! - `/users/me`([GET](users::get_me), [PATCH](users::patch_me))
//! - `/users/{user_id}` ([GET](users::get_user))
//! - `/users/find` ([GET](users::find))
//! - `/legal_votes` ([GET](legal_vote::get_all))
//! - `/legal_votes/{legal_vote_id}` ([GET](legal_vote::get_specific))

pub use request::{CursorPaginationQuery, PagePaginationQuery};
pub use response::{ApiError, ApiResponse, DefaultApiError, DefaultApiResult};

pub mod auth;
pub mod invites;
pub mod legal_vote;
pub mod middleware;
mod request;
mod response;
pub mod rooms;
pub mod sip_configs;
pub mod turn;
pub mod users;

// WWW-Authenticate error-descriptions
static INVALID_ID_TOKEN: &str = "invalid id token";
static INVALID_ACCESS_TOKEN: &str = "invalid access token";
static ACCESS_TOKEN_INACTIVE: &str = "access token inactive";
static SESSION_EXPIRED: &str = "session expired";
