// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::v1::response::error::AuthenticationError;
use crate::api::v1::response::ApiError;
use crate::oidc::{OidcContext, ServiceClaims};
use actix_http::header::Header;
use actix_http::HttpMessage;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::error::Error;
use actix_web::web::Data;
use actix_web::ResponseError;
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use core::future::{ready, Future, Ready};
use core::pin::Pin;
use core::task::{Context, Poll};
use openidconnect::AccessToken;
use std::rc::Rc;
use tracing::Instrument;

/// Contains a list of string representing the service-account's roles in a realm
///
/// Roles can be used to represent certain permissions a service-account has
#[derive(Clone)]
pub struct RealmRoles(Rc<[String]>);

impl RealmRoles {
    pub fn contains(&self, role: &str) -> bool {
        self.0.iter().any(|r| r == role)
    }
}

/// Middleware factory for [`ServiceAuthMiddleware`]
pub struct ServiceAuth {
    oidc_ctx: Data<OidcContext>,
}

impl ServiceAuth {
    pub fn new(oidc_ctx: Data<OidcContext>) -> Self {
        Self { oidc_ctx }
    }
}

impl<S> Transform<S, ServiceRequest> for ServiceAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Transform = ServiceAuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ServiceAuthMiddleware {
            service: Rc::new(service),
            oidc_ctx: self.oidc_ctx.clone(),
        }))
    }
}

/// Middleware which extracts and verifies an access-token from the request
///
/// Inserts a `RealmRoles` struct into the request for other services to inspect.
pub struct ServiceAuthMiddleware<S> {
    service: Rc<S>,

    oidc_ctx: Data<OidcContext>,
}

type ResultFuture<O, E> = Pin<Box<dyn Future<Output = Result<O, E>>>>;

impl<S> Service<ServiceRequest> for ServiceAuthMiddleware<S>
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
        let oidc_ctx = self.oidc_ctx.clone();

        let parse_match_span = tracing::trace_span!("Authorization::<Bearer>::parse");

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
                let realm_roles = check_access_token(oidc_ctx, access_token).await?;
                req.extensions_mut().insert(realm_roles);
                service.call(req).await
            }
            .instrument(tracing::trace_span!("ServiceAuthMiddleware::async::call")),
        )
    }
}

#[tracing::instrument(skip_all)]
async fn check_access_token(
    oidc_ctx: Data<OidcContext>,
    access_token: AccessToken,
) -> Result<RealmRoles, ApiError> {
    let claims = match oidc_ctx.verify_access_token::<ServiceClaims>(&access_token) {
        Ok(claims) => claims,
        Err(e) => {
            log::error!("Invalid access token, {}", e);
            return Err(ApiError::unauthorized()
                .with_www_authenticate(AuthenticationError::InvalidAccessToken));
        }
    };

    let info = match oidc_ctx.introspect_access_token(&access_token).await {
        Ok(info) => info,
        Err(e) => {
            log::error!("Failed to check if AccessToken is active, {}", e);
            return Err(ApiError::internal());
        }
    };

    if info.active {
        let mut realm_roles = claims.realm_access.roles;
        realm_roles
            .iter_mut()
            .for_each(|role| role.make_ascii_lowercase());

        Ok(RealmRoles(realm_roles.into()))
    } else {
        Err(ApiError::unauthorized()
            .with_www_authenticate(AuthenticationError::AccessTokenInactive))
    }
}
