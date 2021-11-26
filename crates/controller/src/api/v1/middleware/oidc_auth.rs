//! Handles user Authentication in API requests
use crate::api::v1::{
    DefaultApiError, ACCESS_TOKEN_INACTIVE, INVALID_ACCESS_TOKEN, SESSION_EXPIRED,
};
use crate::db::users::User;
use crate::oidc::OidcContext;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::error::Error;
use actix_web::http::header::Header;
use actix_web::web::Data;
use actix_web::{HttpMessage, ResponseError};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use core::future::ready;
use database::Db;
use db_storage::DbUsersEx;
use openidconnect::AccessToken;
use std::future::{Future, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::str::FromStr;
use std::task::{Context, Poll};
use uuid::Uuid;

/// Middleware factory
///
/// Transforms into [`OidcAuthMiddleware`]
pub struct OidcAuth {
    pub db_ctx: Data<Db>,
    pub oidc_ctx: Data<OidcContext>,
}

impl<S> Transform<S, ServiceRequest> for OidcAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Transform = OidcAuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(OidcAuthMiddleware {
            service: Rc::new(service),
            db_ctx: self.db_ctx.clone(),
            oidc_ctx: self.oidc_ctx.clone(),
        }))
    }
}

/// Authentication middleware
///
/// Whenever an API request is received, the OidcAuthMiddleware will validate the access
/// token and provide the associated user as [`ReqData`](actix_web::web::ReqData) for the subsequent services.
pub struct OidcAuthMiddleware<S> {
    service: Rc<S>,
    db_ctx: Data<Db>,
    oidc_ctx: Data<OidcContext>,
}

type ResultFuture<O, E> = Pin<Box<dyn Future<Output = Result<O, E>>>>;

impl<S> Service<ServiceRequest> for OidcAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Future = ResultFuture<Self::Response, Self::Error>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let db_ctx = self.db_ctx.clone();
        let oidc_ctx = self.oidc_ctx.clone();

        let auth = match Authorization::<Bearer>::parse(&req) {
            Ok(a) => a,
            Err(e) => {
                log::warn!("Unable to parse access token, {}", e);
                let error = DefaultApiError::auth_bearer_invalid_token(
                    INVALID_ACCESS_TOKEN,
                    "Unable to parse access token".into(),
                );
                let response = req.into_response(error.error_response());
                return Box::pin(ready(Ok(response)));
            }
        };

        let access_token = AccessToken::new(auth.into_scheme().token().to_string());

        Box::pin(async move {
            let current_user = check_access_token(db_ctx, oidc_ctx, access_token).await?;

            req.extensions_mut().insert(current_user);
            service.call(req).await
        })
    }
}

pub async fn check_access_token(
    db_ctx: Data<Db>,
    oidc_ctx: Data<OidcContext>,
    access_token: AccessToken,
) -> Result<User, DefaultApiError> {
    let uuid = match oidc_ctx.verify_access_token(&access_token) {
        Err(e) => {
            log::error!("Invalid access token, {}", e);
            return Err(DefaultApiError::auth_bearer_invalid_token(
                INVALID_ACCESS_TOKEN,
                e.to_string(),
            ));
        }
        Ok(sub) => match Uuid::from_str(&sub) {
            Ok(uuid) => uuid,
            Err(e) => {
                log::error!("Unable to parse UUID from sub '{}', {}", &sub, e);
                return Err(DefaultApiError::auth_bearer_invalid_token(
                    INVALID_ACCESS_TOKEN,
                    "Unable to parse UUID from access token".into(),
                ));
            }
        },
    };

    let current_user = crate::block(move || match db_ctx.get_user_by_uuid(&uuid)? {
        None => {
            log::warn!("The requesting user could not be found in the database");
            Err(DefaultApiError::Internal)
        }
        Some(user) => Ok(user),
    })
    .await
    .map_err(|e| {
        log::error!("crate::block failed, {}", e);
        DefaultApiError::Internal
    })??;

    // check if the id token is expired
    if chrono::Utc::now().timestamp() > current_user.id_token_exp {
        return Err(DefaultApiError::auth_bearer_invalid_token(
            SESSION_EXPIRED,
            "The session for this user has expired".to_string(),
        ));
    }

    let info = match oidc_ctx.introspect_access_token(&access_token).await {
        Ok(info) => info,
        Err(e) => {
            log::error!("Failed to check if AccessToken is active, {}", e);
            return Err(DefaultApiError::Internal);
        }
    };

    if info.active {
        Ok(current_user)
    } else {
        Err(DefaultApiError::auth_bearer_invalid_token(
            ACCESS_TOKEN_INACTIVE,
            "The provided access token is inactive".into(),
        ))
    }
}
