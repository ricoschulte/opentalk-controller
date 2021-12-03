//! Includes an enforcer that supports a background task which reloads the adapter every n seconds
use super::rbac_api_ex::RbacApiEx;
use super::{ToCasbin, ToCasbinString};
use crate::{PolicyUser, UserPolicy};
use async_trait::async_trait;
use casbin::rhai::ImmutableString;
use casbin::{
    Adapter, CoreApi, Effector, EnforceArgs, Enforcer, Event, EventData, Filter, MgmtApi, Model,
    RbacApi, RoleManager, TryIntoAdapter, TryIntoModel,
};
use casbin::{EventEmitter, Result};
use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

type EventCallback = fn(&mut SyncedEnforcer, EventData);

pub struct SyncedEnforcer {
    enforcer: Enforcer,
    events: HashMap<Event, Vec<EventCallback>>,
    autoload_running: bool,
}

impl SyncedEnforcer {
    pub fn is_autoload_running(&self) -> bool {
        self.autoload_running
    }

    #[tracing::instrument(level = "trace", skip(self, user))]
    pub fn get_implicit_resources_for_user<U: Into<PolicyUser>>(
        &mut self,
        user: U,
    ) -> crate::Result<Vec<UserPolicy>> {
        let policy_user: PolicyUser = user.into();
        self.enforcer
            .get_implicit_resources_for_user(&policy_user.to_casbin_string(), None)
            .into_iter()
            .map(TryInto::<UserPolicy>::try_into)
            .map(|r| r.map_err(Into::into))
            .collect()
    }

    /// Checks if a casbin policy of type p is present
    pub fn has_policy<P: ToCasbin>(&self, policy: P) -> bool {
        self.enforcer.has_policy(policy.to_casbin_policy())
    }

    // Checks if a casbin policy of type g is present
    pub fn has_group_policy<P: ToCasbin>(&mut self, policy: P) -> bool {
        let policy = policy.to_casbin_policy();
        self.enforcer
            .has_role_for_user(&policy[0], &policy[1], None)
    }

    /// The autoloader creates the need for using the enforcer inside of a tokio::Mutex
    pub async fn start_autoload_policy(
        enforcer: Arc<RwLock<SyncedEnforcer>>,
        duration: Duration,
        mut shutdown: tokio::sync::broadcast::Receiver<()>,
    ) -> crate::Result<tokio::task::JoinHandle<crate::Result<()>>> {
        let mut write = enforcer.write().await;

        if write.autoload_running {
            return Err(crate::Error::AutoloadRunning);
        }

        write.autoload_running = true;

        let cloned_enforcer = enforcer.clone();

        let handle = tokio::task::spawn_local(async move {
            let mut ticker = tokio::time::interval(duration);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {

                        match cloned_enforcer.write().await.load_policy().await {
                            Ok(_) => {},
                            Err(e) => {
                                return Err(crate::Error::CasbinError(e))
                            },
                        }
                    },
                    _ = shutdown.recv() => {
                        break;
                    }
                }
            }

            cloned_enforcer.write().await.autoload_running = false;

            Ok(())
        });

        Ok(handle)
    }
}

impl EventEmitter<Event> for SyncedEnforcer {
    fn on(&mut self, e: Event, f: fn(&mut Self, EventData)) {
        self.events.entry(e).or_insert_with(Vec::new).push(f)
    }

    fn off(&mut self, e: Event) {
        self.events.remove(&e);
    }

    fn emit(&mut self, e: Event, d: EventData) {
        if let Some(cbs) = self.events.get(&e) {
            for cb in cbs.clone().iter() {
                cb(self, d.clone())
            }
        }
    }
}

#[async_trait]
impl CoreApi for SyncedEnforcer {
    async fn new_raw<M, A>(m: M, a: A) -> Result<SyncedEnforcer>
    where
        M: TryIntoModel,
        A: TryIntoAdapter,
    {
        let enforcer = Enforcer::new_raw(m, a).await?;

        let mut enforcer = SyncedEnforcer {
            enforcer,
            events: HashMap::new(),
            autoload_running: false,
        };

        Ok(enforcer)
    }

    #[inline]
    async fn new<M, A>(m: M, a: A) -> Result<SyncedEnforcer>
    where
        M: TryIntoModel,
        A: TryIntoAdapter,
    {
        let mut enforcer = Self::new_raw(m, a).await?;
        enforcer.load_policy().await?;
        Ok(enforcer)
    }

    #[inline]
    fn add_function(&mut self, fname: &str, f: fn(ImmutableString, ImmutableString) -> bool) {
        self.enforcer.add_function(fname, f);
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self))]
    fn get_model(&self) -> &dyn Model {
        self.enforcer.get_model()
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self))]
    fn get_mut_model(&mut self) -> &mut dyn Model {
        self.enforcer.get_mut_model()
    }

    #[inline]
    fn get_adapter(&self) -> &dyn Adapter {
        self.enforcer.get_adapter()
    }

    #[inline]
    fn get_mut_adapter(&mut self) -> &mut dyn Adapter {
        self.enforcer.get_mut_adapter()
    }

    #[inline]
    fn get_role_manager(&self) -> Arc<parking_lot::RwLock<dyn RoleManager>> {
        self.enforcer.get_role_manager()
    }

    #[inline]
    fn set_role_manager(&mut self, rm: Arc<parking_lot::RwLock<dyn RoleManager>>) -> Result<()> {
        self.enforcer.set_role_manager(rm)
    }

    #[inline]
    async fn set_model<M: TryIntoModel>(&mut self, m: M) -> Result<()> {
        self.enforcer.set_model(m).await
    }

    #[inline]
    async fn set_adapter<A: TryIntoAdapter>(&mut self, a: A) -> Result<()> {
        self.enforcer.set_adapter(a).await
    }

    #[inline]
    fn set_effector(&mut self, e: Box<dyn Effector>) {
        self.enforcer.set_effector(e);
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self, rvals))]
    fn enforce_mut<ARGS: EnforceArgs>(&mut self, rvals: ARGS) -> Result<bool> {
        self.enforcer.enforce_mut(rvals)
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self, rvals))]
    fn enforce<ARGS: EnforceArgs>(&self, rvals: ARGS) -> Result<bool> {
        self.enforcer.enforce(rvals)
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self))]
    fn build_role_links(&mut self) -> Result<()> {
        self.enforcer.build_role_links()
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self, d), fields(event_data = %d))]
    fn build_incremental_role_links(&mut self, d: EventData) -> Result<()> {
        self.enforcer.build_incremental_role_links(d)
    }

    #[inline]
    async fn load_policy(&mut self) -> Result<()> {
        self.enforcer.load_policy().await
    }

    #[inline]
    async fn load_filtered_policy<'a>(&mut self, f: Filter<'a>) -> Result<()> {
        self.enforcer.load_filtered_policy(f).await
    }

    #[inline]
    fn is_filtered(&self) -> bool {
        self.enforcer.is_filtered()
    }

    #[inline]
    fn is_enabled(&self) -> bool {
        self.enforcer.is_enabled()
    }

    #[inline]
    #[tracing::instrument(level = "trace", skip(self))]
    async fn save_policy(&mut self) -> Result<()> {
        self.enforcer.save_policy().await
    }

    #[inline]
    async fn clear_policy(&mut self) -> Result<()> {
        self.enforcer.clear_policy().await
    }

    #[inline]
    fn enable_enforce(&mut self, enabled: bool) {
        self.enforcer.enable_enforce(enabled);
    }

    #[inline]
    fn enable_auto_save(&mut self, auto_save: bool) {
        self.enforcer.enable_auto_save(auto_save);
    }

    #[inline]
    fn enable_auto_build_role_links(&mut self, auto_build_role_links: bool) {
        self.enforcer
            .enable_auto_build_role_links(auto_build_role_links);
    }

    #[inline]
    fn has_auto_save_enabled(&self) -> bool {
        self.enforcer.has_auto_save_enabled()
    }

    #[inline]
    fn has_auto_build_role_links_enabled(&self) -> bool {
        self.enforcer.has_auto_build_role_links_enabled()
    }
}
