// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::{
    access::AccessMethod,
    subject::{IsSubject, PolicyGroup, PolicyRole, PolicyUser},
    ResourceId,
};

/// A policy
///
/// Has a subject, an object, and an action.
/// Can be read as subject can do action on object.
#[derive(Debug, Hash, PartialEq, Eq)]
pub struct Policy<S: IsSubject> {
    pub(crate) sub: S,
    pub(crate) obj: ResourceId,
    pub(crate) act: Vec<AccessMethod>,
}

impl<S: IsSubject> Policy<S> {
    pub fn new<U, D, A>(subject: U, data: D, access: A) -> Self
    where
        U: Into<S>,
        D: Into<ResourceId>,
        A: Into<Vec<AccessMethod>>,
    {
        Self {
            sub: subject.into(),
            obj: data.into(),
            act: access.into(),
        }
    }
}
/// Short version type alias for a Policy with a PolicyUser subject
pub type UserPolicy = Policy<PolicyUser>;

/// Short version type alias for a Policy with a PolicyGroup subject
pub type GroupPolicy = Policy<PolicyGroup>;

/// Short version type alias for a Policy with a PolicyRole subject
pub type RolePolicy = Policy<PolicyRole>;

/// Helper to reduce the amount of boilerplate you need when setting up new permissions
pub struct Policies<'a, T: IsSubject> {
    pub(crate) sub: &'a T,
    pub(crate) data_access: &'a [(&'a ResourceId, &'a [AccessMethod])],
}

pub type UserPolicies<'a> = Policies<'a, PolicyUser>;
pub type GroupPolicies<'a> = Policies<'a, PolicyGroup>;
pub type RolePolicies<'a> = Policies<'a, PolicyRole>;

impl<'a, T: IsSubject> Policies<'a, T> {
    pub fn new(sub: &'a T, data_access: &'a [(&'a ResourceId, &'a [AccessMethod])]) -> Self {
        Self { sub, data_access }
    }
}

impl<T: IsSubject + Clone> From<Policies<'_, T>> for Vec<Policy<T>> {
    fn from(policies: Policies<'_, T>) -> Self {
        policies
            .data_access
            .iter()
            .map(|(obj, acts)| {
                let sub = policies.sub.to_owned();
                let obj: ResourceId = (*obj).clone();
                Policy::<T>::new(sub, obj, *acts)
            })
            .collect()
    }
}
