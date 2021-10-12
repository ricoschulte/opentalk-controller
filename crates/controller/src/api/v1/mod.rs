//! REST API v1
//!
//! Current Endpoints. See their respective function:
//! - `/rooms` ([GET](rooms::owned), [POST](rooms::new))
//! - `/rooms/{room_uuid}` ([GET](rooms::get), [PUT](rooms::modify))
//! - `/rooms/{room_uuid}/start` ([POST](rooms::start))
//! - `/turn` ([GET](turn::get))
//! - `/users` ([GET](users::all))
//! - `/users/me`([GET](users::current_user_profile), [PUT](users::set_current_user_profile))
//! - `/users/{user_id}` ([GET](users::user_details))

pub use request::{CursorPaginationQuery, PagePaginationQuery};
pub use response::{ApiError, ApiResponse, DefaultApiError, DefaultApiResult};

pub mod auth;
pub mod middleware;
mod request;
mod response;
pub mod rooms;
pub mod turn;
pub mod users;

// WWW-Authenticate error-descriptions
static INVALID_ID_TOKEN: &str = "invalid id token";
static INVALID_ACCESS_TOKEN: &str = "invalid access token";
static ACCESS_TOKEN_INACTIVE: &str = "access token inactive";
static SESSION_EXPIRED: &str = "session expired";
