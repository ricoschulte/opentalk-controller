//! Handles user Authentication in API requests
use crate::api::v1::ApiError;
use crate::db::DbInterface;
use crate::oidc::OidcContext;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::error::Error;
use actix_web::http::header::Header;
use actix_web::web::Data;
use actix_web::{web, HttpMessage, ResponseError};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use core::future::ready;
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
    pub db_ctx: Data<DbInterface>,
    pub oidc_ctx: Data<OidcContext>,
}

impl<S, B> Transform<S, ServiceRequest> for OidcAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
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
/// token and provide the associated user as [`ReqData`] for the subsequent services.
pub struct OidcAuthMiddleware<S> {
    service: Rc<S>,
    db_ctx: Data<DbInterface>,
    oidc_ctx: Data<OidcContext>,
}

type ResultFuture<O, E> = Pin<Box<dyn Future<Output = Result<O, E>>>>;

impl<S, B> Service<ServiceRequest> for OidcAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
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
                return Box::pin(ready(Ok(req.into_response(
                    ApiError::auth_error("invalid access token")
                        .error_response()
                        .into_body(),
                ))));
            }
        };

        let access_token = AccessToken::new(auth.into_scheme().token().to_string());

        let uuid = match oidc_ctx.verify_access_token(&access_token) {
            Err(e) => {
                log::warn!("Invalid access token, {}", e);
                return Box::pin(ready(Ok(req.into_response(
                    ApiError::auth_error("invalid access token")
                        .error_response()
                        .into_body(),
                ))));
            }
            Ok(sub) => match Uuid::from_str(&sub) {
                Ok(uuid) => uuid,
                Err(e) => {
                    log::error!("Unable to parse UUID from sub '{}', {}", &sub, e);
                    return Box::pin(ready(Ok(req.into_response(
                        ApiError::auth_error("invalid access token")
                            .error_response()
                            .into_body(),
                    ))));
                }
            },
        };

        Box::pin(async move {
            let current_user = web::block(move || {
                match db_ctx.get_user_by_uuid(&uuid)? {
                    None => {
                        // should only happen if a user gets deleted while in an active session
                        log::error!("The requesting user could not be found in the database");
                        Err(ApiError::Internal)
                    }
                    Some(user) => Ok(user),
                }
            })
            .await
            .map_err(|e| {
                log::error!("BlockingError on Oidc Auth Middleware - {}", e);
                ApiError::Internal
            })??;

            // check if the access token is expired
            if chrono::Utc::now().timestamp() > current_user.id_token_exp {
                return Err(ApiError::auth_error("session expired").into());
            }

            let info = match oidc_ctx.introspect_access_token(&access_token).await {
                Ok(info) => info,
                Err(e) => {
                    log::error!("Failed to check if AccessToken is active, {}", e);
                    return Err(ApiError::Internal.into());
                }
            };

            if info.active {
                req.extensions_mut().insert(current_user);
                service.call(req).await
            } else {
                Err(ApiError::auth_error("invalid access token").into())
            }
        })
    }
}
