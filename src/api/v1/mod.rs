//! REST API
use crate::db::DatabaseError;
use actix_web::body::Body;
use actix_web::http::{header, HeaderValue, StatusCode};
use actix_web::web::BytesMut;
use actix_web::{HttpResponse, Responder, ResponseError};
use std::convert::TryFrom;
use std::fmt::{Display, Formatter, Write};

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
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ApiError {
    /// Contains WWW-Authenticate header (0) & html body (1) for an appropriate 401 response
    #[error("Authentication error: {1}")]
    Auth(WwwAuthHeader, String),
    #[error("The requesting user has insufficient permissions")]
    InsufficientPermission,
    #[error("The requested resource could not be found")]
    NotFound,
    #[error("Bad Request: {0}")]
    BadRequest(String),
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

            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,

            ApiError::ValidationFailed => StatusCode::BAD_REQUEST,

            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse<Body> {
        let mut resp = HttpResponse::new(self.status_code());

        if let Self::Auth(header, _) = self {
            resp.headers_mut()
                .insert(header::WWW_AUTHENTICATE, header.as_header_value());
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
/// The WWW-Authenticate header as specified in RFC6750 & RFC6749
///
/// The Display implementation will convert this struct to a valid WWW-Authenticate header value.
/// Most error codes and variants are not implemented because they are not needed yet.
#[derive(Debug, PartialEq)]
pub struct WwwAuthHeader {
    pub auth_type: AuthenticationType,
    pub error_description: &'static str,
}

impl WwwAuthHeader {
    pub fn new_bearer_invalid_token(error_description: &'static str) -> Self {
        WwwAuthHeader {
            auth_type: AuthenticationType::Bearer(BearerAuthError::InvalidToken),
            error_description,
        }
    }

    pub fn new_bearer_invalid_request(error_description: &'static str) -> Self {
        WwwAuthHeader {
            auth_type: AuthenticationType::Bearer(BearerAuthError::InvalidRequest),
            error_description,
        }
    }

    fn as_header_value(&self) -> HeaderValue {
        let header_result = header::HeaderValue::try_from(self.to_string());

        match header_result {
            Ok(header_value) => header_value,
            Err(e) => {
                log::error!(
                    "Error generating HeaderValue for WWW-Authenticate header '{}', {}",
                    self.to_string(),
                    e
                );

                match self.auth_type {
                    AuthenticationType::Bearer(_) => {
                        header::HeaderValue::from_static(r#"Bearer error="invalid_token""#)
                    }
                }
            }
        }
    }
}

impl Display for WwwAuthHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.error_description.is_empty() {
            write!(f, "{}", self.auth_type)?;
        } else {
            write!(
                f,
                r#"{}, error_description="{}""#,
                self.auth_type, self.error_description
            )?;
        }
        Ok(())
    }
}

/// The WWW-Authenticate authorization type
///
/// The variants contain their respective error variants as fields.
#[derive(Debug, PartialEq)]
pub enum AuthenticationType {
    Bearer(BearerAuthError),
}

impl Display for AuthenticationType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthenticationType::Bearer(e) => write!(f, r#"Bearer error="{}""#, e)?,
        }
        Ok(())
    }
}

/// WWW-Authenticate Bearer errors
#[derive(Debug, PartialEq)]
pub enum BearerAuthError {
    InvalidToken,
    InvalidRequest,
}

impl Display for BearerAuthError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BearerAuthError::InvalidToken => write!(f, "invalid_token"),
            BearerAuthError::InvalidRequest => write!(f, "invalid_request"),
        }
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

// Represents a 204 No Content HTTP Response
pub struct NoContent;

impl Responder for NoContent {
    fn respond_to(self, _: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::NoContent().finish()
    }
}
