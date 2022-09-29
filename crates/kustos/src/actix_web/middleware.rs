//! Actix-web middleware based on <https://github.com/casbin-rs/actix-casbin-auth>
use crate::actix_web::User;
use crate::{AccessMethod, PolicyUser, SyncedEnforcer, UserPolicy};
use actix_web::dev::{Service, Transform};
use actix_web::{
    dev::ServiceRequest, dev::ServiceResponse, Error, HttpMessage, HttpResponse, Result,
};
use casbin::{CoreApi, Result as CasbinResult};
use futures::future::{ok, Ready};
use futures::Future;
use itertools::Itertools;
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::RwLock;
use tracing_futures::Instrument;

#[derive(Clone)]
pub struct KustosService {
    strip_versioned_path: bool,
    enforcer: Arc<RwLock<SyncedEnforcer>>,
}

impl KustosService {
    pub async fn new(
        enforcer: Arc<RwLock<SyncedEnforcer>>,
        strip_versioned_path: bool,
    ) -> CasbinResult<Self> {
        Ok(KustosService {
            strip_versioned_path,
            enforcer,
        })
    }

    pub fn get_enforcer(&self) -> Arc<RwLock<SyncedEnforcer>> {
        self.enforcer.clone()
    }

    pub fn set_enforcer(&self, e: Arc<RwLock<SyncedEnforcer>>) -> KustosService {
        KustosService {
            enforcer: e,
            strip_versioned_path: self.strip_versioned_path,
        }
    }
}

impl<S> Transform<S, ServiceRequest> for KustosService
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type InitError = ();
    type Transform = KustosMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(KustosMiddleware {
            strip_versioned_path: self.strip_versioned_path,
            enforcer: self.enforcer.clone(),
            service: Rc::new(RefCell::new(service)),
        })
    }
}

impl Deref for KustosService {
    type Target = Arc<RwLock<SyncedEnforcer>>;

    fn deref(&self) -> &Self::Target {
        &self.enforcer
    }
}

impl DerefMut for KustosService {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.enforcer
    }
}

type ResultFuture<O, E> = Pin<Box<dyn Future<Output = Result<O, E>>>>;

pub struct KustosMiddleware<S> {
    strip_versioned_path: bool,
    service: Rc<RefCell<S>>,
    enforcer: Arc<RwLock<SyncedEnforcer>>,
}

impl<S> Service<ServiceRequest> for KustosMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse, Error = Error> + 'static,
{
    type Response = ServiceResponse;
    type Error = Error;
    type Future = ResultFuture<Self::Response, Self::Error>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let cloned_enforcer = self.enforcer.clone();
        let srv = self.service.clone();
        let strip_versioned_path = self.strip_versioned_path;
        Box::pin(
            async move {
                let path = if strip_versioned_path {
                    get_unprefixed_path(req.path())?
                } else {
                    req.path().to_string()
                };
                let action = req.method().as_str();
                let action = match AccessMethod::from_str(action) {
                    Ok(action) => action,
                    Err(e) => {
                        log::error!("Invalid method {}", e);
                        return Ok(req.into_response(HttpResponse::Unauthorized().finish()));
                    }
                };

                let option_user = req.extensions().get::<User>().cloned();
                let user = match option_user {
                    Some(value) => value,
                    None => {
                        // TODO(r.floren) This way we might leak the existence of resources. We might want to use NotFound in some cases.
                        log::trace!("No user found. Responding with 401");
                        return Ok(req.into_response(HttpResponse::Unauthorized().finish()));
                    }
                };

                let subject: PolicyUser = user.0.into();
                let lock = cloned_enforcer.read().await;

                let span = tracing::Span::current();

                match lock.enforce(UserPolicy::new(subject, path, action)) {
                    Ok(true) => {
                        drop(lock);
                        span.record("enforcement", true);
                        srv.call(req).await
                    }
                    Ok(false) => {
                        drop(lock);
                        span.record("enforcement", false);
                        Ok(req.into_response(HttpResponse::Forbidden().finish()))
                    }
                    Err(e) => {
                        drop(lock);
                        log::error!("Enforce error {}", e);
                        Ok(req.into_response(HttpResponse::BadGateway().finish()))
                    }
                }
            }
            .instrument(tracing::debug_span!(
                "CasbinMiddleware::async::call",
                enforcement = tracing::field::Empty
            )),
        )
    }
}

/// Returns the unversioned path in case the input path is versioned (i.e. starts with /v{number})
///
/// Way to avoid a regex here
fn get_unprefixed_path(input_path: &str) -> Result<String> {
    let mut segments = input_path.split('/');

    // skip the first segment as its an empty string before the first '/'
    if input_path.starts_with('/') {
        segments.next();
    }

    if let Some(segment) = segments.next() {
        let mut chars = segment.chars();

        if let Some('v') = chars.next() {
            if chars.all(char::is_numeric) {
                // TODO(kbalt): use Split::as_str() when stabilized
                // see https://github.com/rust-lang/rust/issues/77998
                // return Ok(format!("/{}", segments.as_str()));

                return Ok(format!("/{}", segments.join("/")));
            }
        }
    }

    Ok(input_path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use casbin::function_map::key_match2;
    use casbin::prelude::*;
    use casbin::{DefaultModel, Result};

    fn to_owned(v: Vec<&str>) -> Vec<String> {
        v.into_iter().map(ToOwned::to_owned).collect()
    }
    fn to_owned2(v: Vec<Vec<&str>>) -> Vec<Vec<String>> {
        v.into_iter().map(to_owned).collect()
    }

    const MODEL: &str = r#"[request_definition]
    r = sub, dom, obj, act
    
    [policy_definition]
    p = sub, dom, obj, act
    
    [role_definition]
    g = _, _, _
    
    [policy_effect]
    e = some(where (p.eft == allow))
    
    [matchers]
    m = g(r.sub, p.sub, r.dom) && r.dom == p.dom && keyMatch2(r.obj,p.obj) && r.act == p.act || r.sub == "admin"  || r.act == "OPTIONS""#;

    #[tokio::test]
    pub async fn test_policy_conf() {
        let model = DefaultModel::from_str(MODEL).await.unwrap();
        let adapter = casbin::MemoryAdapter::default();

        let mut e = casbin::Enforcer::new(model, adapter).await.unwrap();
        assert!(e
            .add_policies(to_owned2(vec![
                vec!["anonymous", "public", "/api/echo", "POST"],
                vec!["unauthority", "public", "/api/:alive", "GET"],
                vec!["unauthority", "public", "/api/token/create", "POST"],
            ]))
            .await
            .unwrap());
        assert!(e
            .add_grouping_policies(to_owned2(vec![vec!["anonymous", "unauthority", "public"]]))
            .await
            .unwrap());
        assert!(e
            .enforce_mut(("anonymous", "public", "/api/echo", "POST"))
            .unwrap());
        assert!(e
            .enforce_mut(("admin", "publicxx", "/api/xx", "x"))
            .unwrap());
    }

    #[tokio::test]
    async fn test_policy_auth_conf() -> Result<()> {
        let model = DefaultModel::from_str(MODEL).await.unwrap();
        let adapter = casbin::MemoryAdapter::default();
        let enforcer = Arc::new(RwLock::new(SyncedEnforcer::new(model, adapter).await?));

        let casbin_middleware = KustosService::new(enforcer, false).await.unwrap();
        assert!(casbin_middleware
            .get_enforcer()
            .write()
            .await
            .add_policies(to_owned2(vec![
                vec!["anonymous", "public", "/api/echo", "POST"],
                vec!["unauthority", "public", "/api/:alive", "GET"],
                vec!["unauthority", "public", "/api/token/create", "POST"],
            ]))
            .await
            .unwrap());
        assert!(casbin_middleware
            .get_enforcer()
            .write()
            .await
            .add_grouping_policies(to_owned2(vec![vec!["anonymous", "unauthority", "public"]]))
            .await
            .unwrap());
        casbin_middleware
            .write()
            .await
            .get_role_manager()
            .write()
            .set_role_matching_fn(key_match2);

        let share_enforcer = casbin_middleware.get_enforcer();
        let clone_enforcer = share_enforcer.clone();

        assert!(clone_enforcer
            .read()
            .await
            .enforce(("anonymous", "public", "/api/echo", "POST"))
            .unwrap());
        assert!(clone_enforcer
            .read()
            .await
            .enforce(("admin", "publicxx", "/api/xx", "x"))
            .unwrap());
        assert!(!clone_enforcer
            .read()
            .await
            .enforce(("aaadmin", "publicxx", "/api/xx", "x"))
            .unwrap());

        Ok(())
    }

    #[test]
    fn test_get_unprefixed_path() {
        assert_eq!(
            get_unprefixed_path("/v1/rooms").unwrap(),
            "/rooms".to_owned()
        );
        assert_eq!(get_unprefixed_path("/rooms").unwrap(), "/rooms".to_owned());
        assert_eq!(
            get_unprefixed_path("/r2/rooms").unwrap(),
            "/r2/rooms".to_owned()
        );
        assert_eq!(
            get_unprefixed_path("/v1231241a/rooms").unwrap(),
            "/v1231241a/rooms".to_owned()
        );
        assert_eq!(get_unprefixed_path("/").unwrap(), "/".to_owned());
        assert_eq!(get_unprefixed_path("/v23").unwrap(), "/".to_owned());
    }
}
