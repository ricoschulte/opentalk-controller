// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::metrics::EndpointMetrics;
use actix_http::body::{BodySize, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::Error;
use futures::future::{ready, Ready};
use futures::{Future, FutureExt};
use opentelemetry::{Context, Key};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct RequestMetrics {
    metrics: Arc<EndpointMetrics>,
}

impl RequestMetrics {
    pub fn new(metrics: Arc<EndpointMetrics>) -> Self {
        Self { metrics }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RequestMetrics
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = RequestMetricsMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestMetricsMiddleware {
            service,
            metrics: self.metrics.clone(),
        }))
    }
}

const HANDLER_KEY: Key = Key::from_static_str("handler");
const METHOD_KEY: Key = Key::from_static_str("method");
const STATUS_KEY: Key = Key::from_static_str("status");

pub struct RequestMetricsMiddleware<S> {
    service: S,
    metrics: Arc<EndpointMetrics>,
}

impl<S, B> Service<ServiceRequest> for RequestMetricsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<ServiceResponse<B>, Error>>>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let metrics = self.metrics.clone();

        let handler =
            HANDLER_KEY.string(req.match_pattern().unwrap_or_else(|| "default".to_string()));
        let method = METHOD_KEY.string(req.method().to_string());

        let service = self.service.call(req);

        async move {
            let start = Instant::now();

            let result = service.await;

            let duration = start.elapsed();

            let res = if let Ok(resp) = result {
                let status = STATUS_KEY.i64(resp.status().as_u16() as i64);
                let labels = [handler, method, status];

                metrics.request_durations.record(
                    &Context::current(),
                    duration.as_secs_f64(),
                    &labels,
                );

                if let BodySize::Sized(size) = resp.response().body().size() {
                    metrics
                        .response_sizes
                        .record(&Context::current(), size, &labels);
                }

                Ok(resp)
            } else {
                result
            };

            res
        }
        .boxed_local()
    }
}
