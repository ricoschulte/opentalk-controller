//! Kustos (from latin `custos`, means guard)
//!
//! An authorization provider for use in k3k/opentalk.
//! Currently casbin is used as the main library to provide access checks.
//! One benefit over using casbin directly is more typesafety with the added cost of removing some flexibility.
//! This also aims to add an abstraction to be able to switch out the underlying authz framework when needed.
//!
//!
//! This abstraction allows for request-level and resource-level permissions by enforcing a single resource URL (without scheme and authority).
//! For this currently GET, POST, PUT, and DELETE are supported as request-level actions and are also used to enforce resource-level permissions.
//! While the underlying authz framework can use a variance of types Kustos provides strong types for most applications.
//!
//! Subjects:
//! * Users (represented by a UUID)
//! * Groups, user-facing groups (represented by String)
//!   E.g. Groups from a OIDC Provider such as Keycloak
//! * Roles, internal-roles (represented by String)
//!   Like `administrator`, used to provide default super permissions.
//!   The default model allows access to all endpoints for administrators.
//!
//! # Getting Started:
//!
//! 1. Make sure to execute the migrations.
//! 2. Make sure default rules are set. E.g. If `/resource` is a authenticated REST endpoint to add elements to that collection via `POST`,
//!    add the needed rules to allow normal authenticated users to POST to that endpoint to create `resource`s.
//!
//!
//! # Database
//!
//! This crate provides barrel migration files to create the correct database schema for use with the current version.
//! For this call all migrations in the migrations folder in the respective order. You can use a crate like refinery to version your current deployed schema.
//!
//! # Examples:
//!
//! ```rust,no_run
//! # use kustos::prelude::*;
//! # use std::sync::Arc;
//! # use database::Db;
//! # use tokio::runtime::Runtime;
//! # use tokio::task;
//! # use std::time::Duration;
//! # let rt  = Runtime::new().unwrap();
//! # let local = task::LocalSet::new();
//! # local.block_on(&rt, async {
//! let db = Arc::new(
//! Db::connect_url("postgres://postgres:postgres@localhost/kustos", 10, None).unwrap(),
//! );
//! let (mut shutdown, _) = tokio::sync::broadcast::channel(1);
//! let authz = Authz::new_with_autoload(db, shutdown.subscribe(), Duration::from_secs(10))
//!     .await
//!     .unwrap()
//!     .0;
//! authz
//!     .grant_role_access(
//!         "admin",
//!         &[(&ResourceId::from("/users/*"), &[AccessMethod::POST])],
//!     )
//!     .await;
//! authz
//!     .add_user_to_role(uuid::Uuid::nil(), "admin")
//!     .await;
//! authz
//!     .check(
//!         uuid::Uuid::nil(),
//!         "/users/0",
//!         AccessMethod::POST,
//!     )
//!     .await;
//! # });
//! ```
use crate::actix_web::KustosService;
use access::AccessMethod;
use casbin::{CoreApi, MgmtApi, RbacApi};
use db::DbCasbinEx;
use internal::{synced_enforcer::SyncedEnforcer, ToCasbin, ToCasbinMultiple, ToCasbinString};
use policy::{GroupPolicies, Policy, RolePolicies, UserPolicies, UserPolicy};
use std::{str::FromStr, sync::Arc, time::Duration};
use subject::{
    GroupToRole, IsSubject, PolicyGroup, PolicyRole, PolicyUser, UserToGroup, UserToRole,
};
use tokio::{
    sync::{broadcast::Receiver, RwLock},
    task::JoinHandle,
};

pub mod access;
pub mod actix_web;
pub mod db;
mod error;
mod internal;
pub mod policy;
pub mod prelude;
pub mod resource;
pub mod subject;

pub(crate) use error::ParsingError;
pub use error::{Error, ResourceParseError, Result};
pub use resource::*;

#[macro_use]
extern crate diesel;

#[derive(Clone)]
pub struct Authz {
    inner: Arc<RwLock<SyncedEnforcer>>,
}

impl Authz {
    pub async fn new<T>(db_ctx: Arc<T>) -> Result<Self>
    where
        T: 'static + DbCasbinEx + Sync + Send,
    {
        let acl_model = internal::default_acl_model().await;
        let adapter = internal::diesel_adapter::CasbinAdapter::new(db_ctx.clone());
        let enforcer = Arc::new(tokio::sync::RwLock::new(
            SyncedEnforcer::new(acl_model, adapter).await?,
        ));

        Ok(Self { inner: enforcer })
    }

    pub async fn new_with_autoload<T>(
        db_ctx: Arc<T>,
        shutdown_channel: Receiver<()>,
        interval: Duration,
    ) -> Result<(Self, JoinHandle<Result<()>>)>
    where
        T: 'static + DbCasbinEx + Sync + Send,
    {
        let acl_model = internal::default_acl_model().await;
        let adapter = internal::diesel_adapter::CasbinAdapter::new(db_ctx.clone());
        let enforcer = Arc::new(tokio::sync::RwLock::new(
            SyncedEnforcer::new(acl_model, adapter).await?,
        ));

        let handle =
            SyncedEnforcer::start_autoload_policy(enforcer.clone(), interval, shutdown_channel)
                .await?;

        Ok((Self { inner: enforcer }, handle))
    }

    // TODO(r.floren) would be place for a feature gate to use this with other frameworks as well.
    pub async fn actix_web_middleware(&self, strip_versioned_path: bool) -> Result<KustosService> {
        Ok(crate::actix_web::KustosService::new(self.inner.clone(), strip_versioned_path).await?)
    }

    /// Grants the user access to the resources with access
    ///
    /// This takes multiple resources with access at once.
    #[tracing::instrument(level = "debug", skip(self, user, data_access))]
    pub async fn grant_user_access<'a, T>(
        &self,
        user: T,
        data_access: &'a [(&'a ResourceId, &'a [AccessMethod])],
    ) -> Result<()>
    where
        T: Into<PolicyUser>,
    {
        let user = user.into();
        let policies = UserPolicies::new(&user, data_access);

        // The return value of casbins add_policy means: true => newly added, false => already in model.
        // Thus it is sane to just listen for errors here.
        self.inner
            .write()
            .await
            .add_policies(policies.to_casbin_policies())
            .await?;
        Ok(())
    }

    /// Grants the group access to the resources with access
    ///
    /// This takes multiple resources with access at once.
    #[tracing::instrument(level = "debug", skip(self, group, data_access))]
    pub async fn grant_group_access<'a, T>(
        &self,
        group: T,
        data_access: &'a [(&'a ResourceId, &'a [AccessMethod])],
    ) -> Result<()>
    where
        T: Into<PolicyGroup>,
    {
        let group = group.into();
        let policies = GroupPolicies::new(&group, data_access);

        self.inner
            .write()
            .await
            .add_policies(policies.to_casbin_policies())
            .await?;
        Ok(())
    }

    /// Grants the role access to the resources with access
    ///
    /// This takes multiple resources with access at once.
    #[tracing::instrument(level = "debug", skip(self, role, data_access))]
    pub async fn grant_role_access<'a, T>(
        &self,
        role: T,
        data_access: &'a [(&'a ResourceId, &'a [AccessMethod])],
    ) -> Result<()>
    where
        T: Into<PolicyRole>,
    {
        let role = role.into();
        let policies = RolePolicies::new(&role, data_access);

        self.inner
            .write()
            .await
            .add_policies(policies.to_casbin_policies())
            .await?;
        Ok(())
    }

    /// Adds user to group
    #[tracing::instrument(level = "debug", skip(self, user, group))]
    pub async fn add_user_to_group<T, B>(&self, user: T, group: B) -> Result<()>
    where
        T: Into<PolicyUser>,
        B: Into<PolicyGroup>,
    {
        self.inner
            .write()
            .await
            .add_role_for_user(
                &user.into().to_casbin_string(),
                &group.into().to_casbin_string(),
                None,
            )
            .await?;
        Ok(())
    }

    /// Removes user from group
    #[tracing::instrument(level = "debug", skip(self, user, group))]
    pub async fn remove_user_from_group<T, B>(&self, user: T, group: B) -> Result<()>
    where
        T: Into<PolicyUser>,
        B: Into<PolicyGroup>,
    {
        self.inner
            .write()
            .await
            .delete_role_for_user(
                &user.into().to_casbin_string(),
                &group.into().to_casbin_string(),
                None,
            )
            .await?;
        Ok(())
    }

    /// Add user to role
    #[tracing::instrument(level = "debug", skip(self, user, role))]
    pub async fn add_user_to_role<T, B>(&self, user: T, role: B) -> Result<()>
    where
        T: Into<PolicyUser>,
        B: Into<PolicyRole>,
    {
        self.inner
            .write()
            .await
            .add_role_for_user(
                &user.into().to_casbin_string(),
                &role.into().to_casbin_string(),
                None,
            )
            .await?;
        Ok(())
    }

    /// Remove user from role
    #[tracing::instrument(level = "debug", skip(self, user, role))]
    pub async fn remove_user_from_role<T, B>(&self, user: T, role: B) -> Result<()>
    where
        T: Into<PolicyUser>,
        B: Into<PolicyRole>,
    {
        self.inner
            .write()
            .await
            .delete_role_for_user(
                &user.into().to_casbin_string(),
                &role.into().to_casbin_string(),
                None,
            )
            .await?;
        Ok(())
    }

    /// Adds the group to role
    #[tracing::instrument(level = "debug", skip(self, group, role))]
    pub async fn add_group_to_role<T, B>(&self, group: T, role: B) -> Result<()>
    where
        T: Into<PolicyGroup>,
        B: Into<PolicyRole>,
    {
        self.inner
            .write()
            .await
            .add_role_for_user(
                &group.into().to_casbin_string(),
                &role.into().to_casbin_string(),
                None,
            )
            .await?;
        Ok(())
    }

    /// Removes the group from role
    #[tracing::instrument(level = "debug", skip(self, group, role))]
    pub async fn remove_group_from_role<T, B>(&self, group: T, role: B) -> Result<()>
    where
        T: Into<PolicyGroup>,
        B: Into<PolicyRole>,
    {
        self.inner
            .write()
            .await
            .delete_role_for_user(
                &group.into().to_casbin_string(),
                &role.into().to_casbin_string(),
                None,
            )
            .await?;
        Ok(())
    }

    /// Checks if the given user has access to the resource
    #[tracing::instrument(level = "debug", skip(self, user, res, access))]
    pub async fn check<U, R>(&self, user: U, res: R, access: AccessMethod) -> Result<bool>
    where
        U: Into<PolicyUser>,
        R: Into<ResourceId>,
    {
        Ok(self
            .inner
            .read()
            .await
            .enforce(UserPolicy::new(user.into(), res, access))?)
    }

    /// Returns the accessible resource for the given user.
    #[tracing::instrument(level = "debug", skip(self, user, access))]
    pub async fn get_accessible_resources_for_user<U, T>(
        &self,
        user: U,
        access: AccessMethod,
    ) -> Result<AccessibleResources<T>>
    where
        U: Into<PolicyUser>,
        T: Resource + FromStr,
    {
        let policies = self
            .inner
            .write()
            .await
            .get_implicit_resources_for_user(user)?;

        let prefix = T::PREFIX;
        let type_wildcard = format!("{}*", prefix);

        // Check if a wildcard policy is present and fulfills the access request
        let has_wildcard_access = policies
            .iter()
            .find(|x| (*x.obj == type_wildcard || &*x.obj == "/*") && x.act.contains(&access));

        if has_wildcard_access.is_some() {
            return Ok(AccessibleResources::All);
        }

        // Transform the list of accessible resources from strings to T
        let resources = policies
            .into_iter()
            .filter(|x| x.act.contains(&access))
            .map(|x| -> ResourceId { x.into() })
            .filter_map(|x| {
                x.strip_prefix(prefix)
                    // Strip everything after the first occurrence of /
                    .map(|x| x.split_once("/").map(|x| x.0))
                    .flatten()
                    .map(T::from_str)
            })
            .collect::<Result<Vec<T>, <T as FromStr>::Err>>()?;

        Ok(AccessibleResources::List(resources))
    }

    /// Checks if the given permissions is present
    #[tracing::instrument(level = "debug", skip(self, subject, res, access))]
    pub async fn is_permissions_present<T, R, A>(
        &self,
        subject: T,
        res: R,
        access: A,
    ) -> Result<bool>
    where
        T: IsSubject + ToCasbinString,
        R: Into<ResourceId>,
        A: Into<Vec<AccessMethod>>,
    {
        Ok(self
            .inner
            .read()
            .await
            .has_policy(Policy::<T>::new(subject, res, access)))
    }

    /// Checks if the given group is in the given role
    #[tracing::instrument(level = "debug", skip(self, group, role))]
    pub async fn is_group_in_role<G, R>(&self, group: G, role: R) -> Result<bool>
    where
        G: Into<PolicyGroup>,
        R: Into<PolicyRole>,
    {
        Ok(self
            .inner
            .write()
            .await
            .has_group_policy(GroupToRole(group.into(), role.into())))
    }

    /// Checks if the given user is in the given group
    #[tracing::instrument(level = "debug", skip(self, user, group))]
    pub async fn is_user_in_group<G, R>(&self, user: G, group: R) -> Result<bool>
    where
        G: Into<PolicyUser>,
        R: Into<PolicyGroup>,
    {
        Ok(self
            .inner
            .write()
            .await
            .has_group_policy(UserToGroup(user.into(), group.into())))
    }

    /// Checks if the given user is in the given role
    #[tracing::instrument(level = "debug", skip(self, user, role))]
    pub async fn is_user_in_role<G, R>(&self, user: G, role: R) -> Result<bool>
    where
        G: Into<PolicyUser>,
        R: Into<PolicyRole>,
    {
        Ok(self
            .inner
            .write()
            .await
            .has_group_policy(UserToRole(user.into(), role.into())))
    }

    /// Removes all rules that explicitly name the given resource
    #[tracing::instrument(level = "debug", skip(self, resource))]
    pub async fn remove_explicit_resource_permissions(&self, resource: String) -> Result<bool> {
        self.inner
            .write()
            .await
            .remove_filtered_policy(1, vec![resource])
            .await
            .map_err(Into::into)
    }

    /// Removes access for user
    #[tracing::instrument(level = "debug", skip(self, user, resource, access))]
    pub async fn remove_user_permission<S, R, A>(
        &self,
        user: S,
        resource: R,
        access: A,
    ) -> Result<bool>
    where
        S: Into<PolicyUser>,
        R: Into<ResourceId>,
        A: Into<Vec<AccessMethod>>,
    {
        self.inner
            .write()
            .await
            .remove_policy(
                Policy::<PolicyUser>::new(user.into(), resource.into(), access.into())
                    .to_casbin_policy(),
            )
            .await
            .map_err(Into::into)
    }

    /// Removes access for role
    #[tracing::instrument(level = "debug", skip(self, user, resource, access))]
    pub async fn remove_role_permission<S, R, A>(
        &self,
        user: S,
        resource: R,
        access: A,
    ) -> Result<bool>
    where
        S: Into<PolicyRole>,
        R: Into<ResourceId>,
        A: Into<Vec<AccessMethod>>,
    {
        self.inner
            .write()
            .await
            .remove_policy(
                Policy::<PolicyRole>::new(user.into(), resource.into(), access.into())
                    .to_casbin_policy(),
            )
            .await
            .map_err(Into::into)
    }
}
