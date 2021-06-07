//! REST API
use crate::db::DatabaseError;
use actix_web::body::Body;
use actix_web::http::{header, StatusCode};
use actix_web::web::BytesMut;
use actix_web::{BaseHttpResponse, ResponseError};
use std::convert::TryFrom;
use std::fmt::Write;

pub mod auth;
pub mod middleware;
pub mod rooms;
pub mod turn;
pub mod users;

// WWW-Authenticate error-descriptions
static INVALID_ID_TOKEN: &str = "invalid id token";
static INVALID_ACCESS_TOKEN: &str = "invalid access token";
static ACCESS_TOKEN_INACTIVE: &str = "access token inactive";
static SESSION_EXPIRED: &str = "session expired";

/// Error type of all frontend REST-endpoints
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Contains WWW-Authenticate error desc (0) & body (1) for an appropriate 401 response
    #[error("Authentication error: {1}")]
    Auth(&'static str, String),
    #[error("The requesting user has insufficient permissions")]
    InsufficientPermission,
    #[error("The requested resource could not be found")]
    NotFound,
    #[error("The provided JSON object does not follow the specified field constraints")]
    ValidationFailed,
    #[error("An internal server error occurred")]
    Internal,
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Auth(_, _) => StatusCode::UNAUTHORIZED,

            ApiError::InsufficientPermission => StatusCode::FORBIDDEN,

            ApiError::NotFound => StatusCode::NOT_FOUND,

            ApiError::ValidationFailed => StatusCode::BAD_REQUEST,

            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> BaseHttpResponse<Body> {
        let mut resp = BaseHttpResponse::new(self.status_code());

        if let Self::Auth(header_desc, _) = self {
            let header_result = header::HeaderValue::try_from(format!(
                "Bearer error=\"invalid_token\", error_description=\"{}\"",
                header_desc
            ));

            let header_value = match header_result {
                Ok(header_value) => header_value,
                Err(e) => {
                    log::debug!(
                        "Error generating WWW-Authenticate header with error description '{}', {}",
                        header_desc,
                        e
                    );
                    header::HeaderValue::from_static("Bearer error=\"invalid_token\"")
                }
            };

            resp.headers_mut()
                .insert(header::WWW_AUTHENTICATE, header_value);
        }

        let mut buf = BytesMut::new();
        let _ = write!(buf, "{}", self);

        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("text/plain; charset=utf-8"),
        );

        resp.set_body(Body::from(buf))
    }
}

impl From<actix_web::Error> for ApiError {
    fn from(_: actix_web::Error) -> Self {
        Self::Internal
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(_: anyhow::Error) -> Self {
        Self::Internal
    }
}

impl From<crate::db::DatabaseError> for ApiError {
    fn from(e: DatabaseError) -> Self {
        match e {
            DatabaseError::NotFound => Self::NotFound,
            _ => Self::Internal,
        }
    }
}
