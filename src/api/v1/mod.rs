//! REST API
use crate::db::DatabaseError;
use actix_web::body::Body;
use actix_web::http::{header, HeaderValue, StatusCode};
use actix_web::{HttpResponse, Responder, ResponseError};
use actix_web_httpauth::headers::www_authenticate::bearer::{Bearer, Error};
use actix_web_httpauth::headers::www_authenticate::Challenge;
use serde::Serialize;
use std::fmt;

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

/// The default ApiError
pub type DefaultApiError = ApiError<StandardErrorKind>;

/// Error type of all frontend REST-endpoints
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ApiError<E = StandardErrorKind>
where
    E: fmt::Debug + Serialize,
{
    /// Contains a bearer WWW-Authenticate header (0) & html body (1) for an 401 response
    #[error("Authentication error: {1}")]
    Auth(Bearer, String),
    /// A 401 response without the WWW-Authenticate header, contains a serializable json body
    #[error("{0}")]
    AuthJson(ErrorBody<E>),
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

/// The standard error type for the [`ApiError`]
#[derive(Debug, Serialize)]
pub enum StandardErrorKind {}

/// Used by [`ApiError`] variants that contain a JSON body.
///
/// Uses the Display implementation to serialize.
#[derive(Debug, Serialize, PartialEq)]
pub struct ErrorBody<E> {
    error: E,
}

impl<E> From<E> for ErrorBody<E>
where
    E: fmt::Debug + Serialize,
{
    fn from(error: E) -> Self {
        Self { error }
    }
}

impl<E> fmt::Display for ErrorBody<E>
where
    E: fmt::Debug + Serialize,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string(self).map_err(|_| fmt::Error)?
        )
    }
}

impl<E> ResponseError for ApiError<E>
where
    E: fmt::Debug + Serialize,
{
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Auth(_, _) => StatusCode::UNAUTHORIZED,

            Self::AuthJson(_) => StatusCode::UNAUTHORIZED,

            Self::InsufficientPermission => StatusCode::FORBIDDEN,

            Self::NotFound => StatusCode::NOT_FOUND,

            Self::BadRequest(_) => StatusCode::BAD_REQUEST,

            Self::ValidationFailed => StatusCode::BAD_REQUEST,

            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse<Body> {
        let mut response = HttpResponse::new(self.status_code());

        match self {
            Self::Auth(bearer, _) => {
                let header_value = match HeaderValue::from_maybe_shared(bearer.to_bytes()) {
                    Ok(header_value) => header_value,
                    Err(e) => {
                        log::error!(
                            "Error generating HeaderValue for WWW-Authenticate bearer '{:?}', {}",
                            bearer,
                            e
                        );
                        header::HeaderValue::from_static(r#"Bearer error="invalid_request""#)
                    }
                };

                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, header_value);

                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/plain; charset=utf-8"),
                );

                response.set_body(Body::from(self.to_string()))
            }
            Self::AuthJson(json) => {
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/json; charset=utf-8"),
                );

                response.set_body(Body::from(json.to_string()))
            }
            _ => {
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/plain; charset=utf-8"),
                );

                response.set_body(Body::from(self.to_string()))
            }
        }
    }
}

impl DefaultApiError {
    /// Create an invalid token [`ApiError::Auth`] error
    ///
    /// Resolves to the following WWW-Authenticate header:
    ///
    /// ```
    /// Bearer error="invalid_token", error_description="<error_description>"
    /// ```
    ///
    /// The `response_body` will be sent as a plaintext html body.
    pub fn auth_bearer_invalid_token(
        error_description: &'static str,
        response_body: String,
    ) -> DefaultApiError {
        DefaultApiError::Auth(
            Bearer::build()
                .error(Error::InvalidToken)
                .error_description(error_description)
                .finish(),
            response_body,
        )
    }

    /// Create an invalid request [`ApiError::Auth`] error
    ///
    /// Resolves to the following WWW-Authenticate header:
    ///
    /// ```
    /// Bearer error="invalid_request", error_description="<error_description>"
    /// ```
    ///
    /// The `response_body` will be sent as a plaintext html body.
    pub fn auth_bearer_invalid_request(
        error_description: &'static str,
        response_body: String,
    ) -> DefaultApiError {
        DefaultApiError::Auth(
            Bearer::build()
                .error(Error::InvalidRequest)
                .error_description(error_description)
                .finish(),
            response_body,
        )
    }
}

impl<E> From<actix_web::Error> for ApiError<E>
where
    E: fmt::Debug + Serialize,
{
    fn from(_: actix_web::Error) -> Self {
        Self::Internal
    }
}

impl<E> From<anyhow::Error> for ApiError<E>
where
    E: fmt::Debug + Serialize,
{
    fn from(_: anyhow::Error) -> Self {
        Self::Internal
    }
}

impl<E> From<crate::db::DatabaseError> for ApiError<E>
where
    E: fmt::Debug + Serialize,
{
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
