use crate::api::v1::middleware::oidc_auth::check_access_token;
use crate::oidc::OidcContext;
use actix_web::web::{Data, Json};
use actix_web::{post, HttpResponse};
use database::Db;
use openidconnect::{AccessToken, RefreshToken};
use serde::{Deserialize, Serialize};

/// The JSON Body expected when making a *POST* request on `/introspect`
#[derive(Debug, Deserialize)]
pub struct IntrospectRequest {
    token: String,
    /// TypeHint
    ///
    /// Expected values: refresh_token, access_token
    token_type_hint: Option<String>,
}

/// The JSON Body returned on by `/introspect`
#[derive(Debug, Serialize)]
pub struct IntrospectResponse {
    active: bool,
    sub: Option<String>,
}

#[derive(Debug)]
enum TokenType {
    Access,
    Refresh,
    Unknown,
}

/// API Endpoint *POST /introspect*
///
/// Verifies that the JWT `token` inside the provided [`Json<Introspect>`] body is valid and active.
/// see specification: (RFC7662)<https://datatracker.ietf.org/doc/html/rfc7662>
/// This a minimal implementation for access tokens
///
#[post("/introspect")]
pub async fn introspect(
    body: Json<IntrospectRequest>,
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
) -> HttpResponse {
    let request = body.into_inner();
    let token_type = match request.token_type_hint.as_deref() {
        Some("access_token") => TokenType::Access,
        Some("refresh_token") => TokenType::Refresh,
        _ => TokenType::Unknown,
        // TODO run auto detect:
        // parse jwt => token.typ is eiher 'Bearer' or 'Refresh'
    };

    match token_type {
        TokenType::Access => {
            introspect_access_token(AccessToken::new(request.token), db, oidc_ctx).await
        }
        TokenType::Refresh => {
            introspect_refresh_token(RefreshToken::new(request.token), db, oidc_ctx).await
        }
        TokenType::Unknown => {
            introspect_access_token(AccessToken::new(request.token), db, oidc_ctx).await
        } //try access_token decoding if we don't know
    }
}

async fn introspect_access_token(
    token: AccessToken,
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
) -> HttpResponse {
    match check_access_token(db, oidc_ctx, token).await {
        Err(_e) => HttpResponse::Ok().json(IntrospectResponse {
            active: false,
            sub: None,
        }),
        Ok(user) => {
            //TODO get AccessTokenIntrospectInfo
            HttpResponse::Ok().json(IntrospectResponse {
                active: true,
                sub: Some(user.oidc_sub),
            })
        }
    }
}

async fn introspect_refresh_token(
    _token: RefreshToken,
    _db: Data<Db>,
    _oidc_ctx: Data<OidcContext>,
) -> HttpResponse {
    log::error!("Refresh token introspection not implemented.");
    HttpResponse::NotImplemented().body("Refresh token introspection not implemented.")
}
