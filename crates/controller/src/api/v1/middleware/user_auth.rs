// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Handles user Authentication in API requests
use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::ApiError;
use crate::oidc::{OidcContext, UserClaims};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::error::Error;
use actix_web::http::header::Header;
use actix_web::web::Data;
use actix_web::{HttpMessage, ResponseError};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use controller_shared::settings::{Settings, SharedSettings, TenantAssignment};
use core::future::ready;
use database::Db;
use db_storage::tenants::{OidcTenantId, Tenant};
use db_storage::users::User;
use openidconnect::AccessToken;
use std::future::{Future, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use tracing_futures::Instrument;

/// Middleware factory
///
/// Transforms into [`OidcAuthMiddleware`]
pub struct OidcAuth {
    pub settings: SharedSettings,
    pub db: Data<Db>,
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
            settings: self.settings.clone(),
            db: self.db.clone(),
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
    settings: SharedSettings,
    db: Data<Db>,
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
        let settings = self.settings.clone();
        let db = self.db.clone();
        let oidc_ctx = self.oidc_ctx.clone();

        let parse_match_span =
            tracing::span!(tracing::Level::TRACE, "Authorization::<Bearer>::parse");

        let _enter = parse_match_span.enter();
        let auth = match Authorization::<Bearer>::parse(&req) {
            Ok(a) => a,
            Err(e) => {
                log::warn!("Unable to parse access token, {}", e);
                let error = ApiError::unauthorized()
                    .with_message("Unable to parse access token")
                    .with_www_authenticate(AuthenticationError::InvalidAccessToken);

                let response = req.into_response(error.error_response());
                return Box::pin(ready(Ok(response)));
            }
        };

        let access_token = AccessToken::new(auth.into_scheme().token().to_string());

        Box::pin(
            async move {
                let settings = settings.load_full();
                let (current_tenant, current_user) =
                    check_access_token(&settings, db, oidc_ctx, access_token).await?;
                req.extensions_mut()
                    .insert(kustos::actix_web::User::from(current_user.id.into_inner()));
                req.extensions_mut().insert(current_tenant);
                req.extensions_mut().insert(current_user);
                service.call(req).await
            }
            .instrument(tracing::trace_span!("OidcAuthMiddleware::async::call")),
        )
    }
}

#[tracing::instrument(skip_all)]
pub async fn check_access_token(
    settings: &Settings,
    db: Data<Db>,
    oidc_ctx: Data<OidcContext>,
    access_token: AccessToken,
) -> Result<(Tenant, User), ApiError> {
    let (oidc_tenant_id, sub) = match oidc_ctx.verify_access_token::<UserClaims>(&access_token) {
        Ok(claims) => {
            // Get the tenant_id depending on the configured assignment
            let tenant_id = match &settings.tenants.assignment {
                TenantAssignment::Static { static_tenant_id } => static_tenant_id.clone(),
                TenantAssignment::ByExternalTenantId => claims.tenant_id.ok_or_else(|| {
                    log::error!("Invalid access token, missing tenant_id");
                    ApiError::unauthorized()
                        .with_www_authenticate(AuthenticationError::InvalidAccessToken)
                })?,
            };

            (tenant_id, claims.sub)
        }
        Err(e) => {
            log::error!("Invalid access token, {}", e);
            return Err(ApiError::unauthorized()
                .with_www_authenticate(AuthenticationError::InvalidAccessToken));
        }
    };

    let (current_tenant, current_user) = crate::block(move || {
        let mut conn = db.get_conn()?;

        let tenant = Tenant::get_by_oidc_id(&mut conn, OidcTenantId::from(oidc_tenant_id))?
            .ok_or_else(|| {
                ApiError::unauthorized()
                    .with_code("unknown_tenant_id")
                    .with_message("Unknown tenant_id in access token. Please login first!")
            })?;

        match User::get_by_oidc_sub(&mut conn, tenant.id, &sub)? {
            Some(user) => Ok((tenant, user)),
            None => Err(ApiError::unauthorized()
                .with_code("unknown_sub")
                .with_message("Unknown subject in access token. Please login first!")),
        }
    })
    .await??;

    // check if the id token is expired
    if chrono::Utc::now().timestamp() > current_user.id_token_exp {
        return Err(ApiError::unauthorized()
            .with_message("The session for this user has expired")
            .with_www_authenticate(AuthenticationError::SessionExpired));
    }

    let info = match oidc_ctx.introspect_access_token(&access_token).await {
        Ok(info) => info,
        Err(e) => {
            log::error!("Failed to check if AccessToken is active, {}", e);
            return Err(ApiError::internal());
        }
    };

    if info.active {
        Ok((current_tenant, current_user))
    } else {
        Err(ApiError::unauthorized()
            .with_www_authenticate(AuthenticationError::AccessTokenInactive))
    }
}
