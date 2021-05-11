use crate::oidc::OidcContext;
use actix_web::http::StatusCode;
use actix_web::web::Data;
use actix_web::{post, FromRequest, HttpRequest, HttpResponse, ResponseError};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::extractors::AuthenticationError;
use actix_web_httpauth::headers::www_authenticate::bearer::Bearer;
use anyhow::Result;
use openidconnect::AccessToken;

pub mod auth;

/// Error type of all frontend REST-endpoints
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("missing authentication")]
    AuthenticationError(#[from] AuthenticationError<Bearer>),
    #[error("invalid access token")]
    InvalidAccessToken,
    #[error("invalid id token")]
    InvalidIdToken,
    #[error("internal server error")]
    Internal,
    #[error("internal server error")]
    Actix(#[from] actix_web::Error),
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::AuthenticationError(_) => StatusCode::UNAUTHORIZED,

            ApiError::InvalidAccessToken => StatusCode::FORBIDDEN,
            ApiError::InvalidIdToken => StatusCode::FORBIDDEN,

            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Actix(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[post("/api_example")]
pub async fn api_example(
    request: HttpRequest,
    oidc_ctx: Data<OidcContext>,
) -> Result<HttpResponse, ApiError> {
    verify_access_token(&request, &oidc_ctx).await?;

    Ok(HttpResponse::Ok().await?)
}

/// Verifies if a `HttpRequest` contains a valid AccessToken inside its `authorization` header.
async fn verify_access_token(
    request: &HttpRequest,
    oidc_ctx: &OidcContext,
) -> Result<(), ApiError> {
    let auth = BearerAuth::extract(&request).await?;
    let access_token = AccessToken::new(auth.token().into());

    let _sub = match oidc_ctx.verify_access_token(&access_token) {
        Err(e) => {
            log::warn!("Invalid access token, {}", e);
            return Err(ApiError::InvalidAccessToken);
        }
        Ok(sub) => sub,
    };

    // TODO check id token validity inside DB using sub

    let info = match oidc_ctx.introspect_access_token(&access_token).await {
        Ok(info) => info,
        Err(e) => {
            log::error!("Failed to check if AccessToken is active, {}", e);
            return Err(ApiError::Internal);
        }
    };

    if info.active {
        Ok(())
    } else {
        Err(ApiError::InvalidAccessToken)
    }
}
