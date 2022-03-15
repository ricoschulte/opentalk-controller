use crate::policy::{GroupPolicy, Policy, RolePolicy, UserPolicy};
use crate::prelude::{PolicyGroup, PolicyRole, PolicyUser};
use crate::{AccessMethod, IsSubject, ResourceId};
use std::fmt::Debug;
use std::mem::take;

#[doc(hidden)]
pub trait BuilderState: Debug {
    fn finalize<B>(self, policies: &mut PoliciesBuilder<B>);
}

/// A builder to build multiple policies at once
///
/// This uses an internal BuilderState to distinguish where to add a policy (user, group, role)
///
/// # Example:
/// ```
/// # use uuid::Uuid;
/// # use kustos::prelude::*;
/// let user_id = Uuid::nil();
/// let policies = PoliciesBuilder::default()
///     .grant_user_access(user_id)
///     .add_resource("/rooms/x", [AccessMethod::Get])
///     .finish();
/// ```
#[derive(Debug)]
pub struct PoliciesBuilder<B> {
    pub(crate) user_policies: Vec<UserPolicy>,
    pub(crate) group_policies: Vec<GroupPolicy>,
    pub(crate) role_policies: Vec<RolePolicy>,
    state: B,
}

/// Initial state of the builder
#[derive(Debug)]
pub struct Empty;

impl BuilderState for Empty {
    fn finalize<B>(self, _: &mut PoliciesBuilder<B>) {}
}

impl PoliciesBuilder<Empty> {
    /// Creates a new PoliciesBuilder in the `Empty` state
    pub fn new() -> Self {
        Self {
            user_policies: vec![],
            group_policies: vec![],
            role_policies: vec![],
            state: Empty,
        }
    }
}

impl Default for PoliciesBuilder<Empty> {
    fn default() -> Self {
        Self::new()
    }
}

/// Final state of the builder, only in this state may it be serialized
#[derive(Debug)]
pub struct Finished;

impl<B: BuilderState> PoliciesBuilder<B> {
    /// Wraps up the previous state and switches to granting access to the given user
    ///
    /// Until you call another `grant_*_access` method or `finish`, all following [`add_resource`] calls will target this user.
    pub fn grant_user_access<T>(self, user: T) -> PoliciesBuilder<GrantingAccess<PolicyUser>>
    where
        T: Into<PolicyUser>,
    {
        let mut this = PoliciesBuilder {
            user_policies: self.user_policies,
            group_policies: self.group_policies,
            role_policies: self.role_policies,
            state: GrantingAccess::new(user.into()),
        };

        self.state.finalize(&mut this);

        this
    }

    /// Wraps up the previous state and switches to granting access to the given group
    ///
    /// Until you call another `grant_*_access` method or `finish`, all following [`add_resource`] calls will target this group.
    pub fn grant_group_access<T>(self, group: T) -> PoliciesBuilder<GrantingAccess<PolicyGroup>>
    where
        T: Into<PolicyGroup>,
    {
        let mut this = PoliciesBuilder {
            user_policies: self.user_policies,
            group_policies: self.group_policies,
            role_policies: self.role_policies,
            state: GrantingAccess::new(group.into()),
        };

        self.state.finalize(&mut this);

        this
    }

    /// Wraps up the previous state and switches to granting access to the given role
    ///
    /// Until you call another `grant_*_access` method or [`finish`], all fallowing [`add_resource`] calls will target this role.
    pub fn grant_role_access<T>(self, role: T) -> PoliciesBuilder<GrantingAccess<PolicyRole>>
    where
        T: Into<PolicyRole>,
    {
        let mut this = PoliciesBuilder {
            user_policies: self.user_policies,
            group_policies: self.group_policies,
            role_policies: self.role_policies,
            state: GrantingAccess::new(role.into()),
        };

        self.state.finalize(&mut this);

        this
    }

    /// Wraps up the previous state and switched to the `Finished` state
    ///
    /// Only in the `Finished` state can the builder be passed to the [`add_policies`](super::Authz::add_policies)
    /// function of [Authz](super::Authz).
    pub fn finish(self) -> PoliciesBuilder<Finished> {
        let mut this = PoliciesBuilder {
            user_policies: self.user_policies,
            group_policies: self.group_policies,
            role_policies: self.role_policies,
            state: Finished,
        };

        self.state.finalize(&mut this);

        this
    }
}

impl<T> PoliciesBuilder<GrantingAccess<T>>
where
    T: IsSubject + Clone,
{
    /// Add a resource to grant the given access to
    pub fn add_resource<R, A>(mut self, resource: R, access: A) -> Self
    where
        R: Into<ResourceId>,
        A: Into<Vec<AccessMethod>> + IntoIterator<Item = AccessMethod>,
    {
        let resource = resource.into();

        if let Some((_, al)) = self
            .state
            .resources
            .iter_mut()
            .find(|(r, _)| *r == resource)
        {
            for access in access {
                if !al.contains(&access) {
                    al.push(access);
                }
            }
        } else {
            self.state.resources.push((resource, access.into()));
        }

        self
    }
}

/// State of granting access of multiple resource to a single subject (e.g. user, group or role)
#[derive(Debug)]
pub struct GrantingAccess<T: IsSubject + Clone> {
    sub: T,
    resources: Vec<(ResourceId, Vec<AccessMethod>)>,
}

impl<T: IsSubject + Clone> GrantingAccess<T> {
    fn new(sub: T) -> Self {
        Self {
            sub,
            resources: vec![],
        }
    }

    fn resources_iter(mut self) -> impl Iterator<Item = Policy<T>> {
        let sub = self.sub.clone();

        take(&mut self.resources)
            .into_iter()
            .map(move |(resource, access)| -> Policy<T> {
                Policy::<T>::new(sub.clone(), resource, access)
            })
    }
}

impl BuilderState for GrantingAccess<PolicyUser> {
    fn finalize<B>(self, policies: &mut PoliciesBuilder<B>) {
        policies.user_policies.extend(self.resources_iter());
    }
}

impl BuilderState for GrantingAccess<PolicyGroup> {
    fn finalize<B>(self, policies: &mut PoliciesBuilder<B>) {
        policies.group_policies.extend(self.resources_iter());
    }
}

impl BuilderState for GrantingAccess<PolicyRole> {
    fn finalize<B>(self, policies: &mut PoliciesBuilder<B>) {
        policies.role_policies.extend(self.resources_iter());
    }
}
