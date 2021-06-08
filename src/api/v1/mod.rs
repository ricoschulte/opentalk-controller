//! REST API
use crate::db::DatabaseError;
use actix_web::body::Body;
use actix_web::http::{header, StatusCode};
use actix_web::web::BytesMut;
use actix_web::{BaseHttpResponse, ResponseError};
use actix_web_httpauth::extractors::bearer::Error;
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::headers::www_authenticate::bearer::Bearer;
use std::fmt::Write;

pub mod auth;
pub mod middleware;
pub mod rooms;
pub mod turn;
pub mod users;

/// Error type of all frontend REST-endpoints
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Authentication error")]
    Auth(#[from] AuthenticationError<Bearer>),
    #[error("Insufficient permissions")]
    InsufficientPermission,
    #[error("Not found")]
    NotFound,
    #[error("The provided object does not follow the specified field constraints")]
    ValidationFailed,
    #[error("Internal server error")]
    Internal,
}

impl ApiError {
    fn auth_error(desc: &'static str) -> Self {
        let bearer = Bearer::build()
            .error(Error::InvalidToken)
            .error_description(desc)
            .finish();

        Self::Auth(AuthenticationError::new(bearer))
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Auth(_) => StatusCode::UNAUTHORIZED,

            ApiError::InsufficientPermission => StatusCode::FORBIDDEN,

            ApiError::NotFound => StatusCode::NOT_FOUND,

            ApiError::ValidationFailed => StatusCode::BAD_REQUEST,

            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> BaseHttpResponse<Body> {
        if let Self::Auth(e) = self {
            return e.error_response();
        }

        let mut resp = BaseHttpResponse::new(self.status_code());

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
