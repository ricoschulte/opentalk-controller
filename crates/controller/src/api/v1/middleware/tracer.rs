//! Handles request tracing

use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::error::Error;
use futures::future::{ready, Ready};
use std::task::{Context, Poll};
use tracing::instrument::Instrumented;
use tracing::Instrument;

/// Middleware factory
///
/// Transforms into [`TracerMiddleWare`]
pub struct Tracer;

impl<S> Transform<S, ServiceRequest> for Tracer
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Transform = TracerMiddleWare<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(TracerMiddleWare { service }))
    }
}

/// Tracing middleware
///
/// Creates a span for each request containing the request method and uri
pub struct TracerMiddleWare<S> {
    service: S,
}

impl<S> Service<ServiceRequest> for TracerMiddleWare<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
    S::Future: 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Future = Instrumented<S::Future>;

    fn poll_ready(&self, ctx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let span = tracing::info_span!(
            "request",
            method = %req.head().method,
            uri = %req.head().uri
        );

        let guard = span.enter();

        log::debug!(
            target: "k3k_controller::api",
            "Incoming request {} {}",
            req.head().method,
            req.head().uri,
        );

        drop(guard);

        self.service.call(req).instrument(span)
    }
}
