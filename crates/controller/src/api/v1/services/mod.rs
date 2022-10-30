// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::middleware::service_auth::RealmRoles;
use super::response::ApiError;
use actix_http::HttpMessage;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::Error;
use core::future::{ready, Ready};
use core::task::{Context, Poll};
use futures::future::Either;
use std::rc::Rc;

pub mod call_in;
pub mod recording;

/// Middleware factory for [`RequiredRealmRoleMiddleware`]
struct RequiredRealmRole {
    role: Rc<str>,
}

impl RequiredRealmRole {
    fn new(role: impl Into<Rc<str>>) -> Self {
        Self { role: role.into() }
    }
}

impl<S> Transform<S, ServiceRequest> for RequiredRealmRole
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Transform = RequiredRealmRoleMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequiredRealmRoleMiddleware {
            service,
            role: self.role.clone(),
        }))
    }
}

/// Checks if the request has [`RealmRoles`] and they contain a certain role
///
/// If it doesn't contain the required role, it returns a [`ApiError::unauthorized`]
struct RequiredRealmRoleMiddleware<S> {
    service: S,
    role: Rc<str>,
}

impl<S> Service<ServiceRequest> for RequiredRealmRoleMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error>,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Future = Either<S::Future, Ready<Result<Self::Response, Self::Error>>>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let allowed = {
            let extensions = req.extensions();
            let roles = extensions.get::<RealmRoles>();
            roles
                .map(|roles| roles.contains(&self.role))
                .unwrap_or_else(|| {
                    log::warn!("`RequiredRealmRoleMiddleware` requires the requests to contain a `RealmRoles`");
                    false
                })
        };

        if allowed {
            Either::Left(self.service.call(req))
        } else {
            Either::Right(ready(Err(Error::from(ApiError::unauthorized()))))
        }
    }
}
