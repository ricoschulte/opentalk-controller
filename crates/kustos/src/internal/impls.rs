//! Provides various internal implementations
use super::{ToCasbin, ToCasbinMultiple, ToCasbinString};
use crate::{
    policies_builder::{Finished, PoliciesBuilder},
    policy::{Policies, Policy},
    subject::{UserToGroup, UserToRole},
    AccessMethod, GroupToRole, IsSubject, PolicyGroup, PolicyRole, PolicyUser, ResourceId,
    UserPolicy,
};
use casbin::{rhai::Dynamic, EnforceArgs};
use itertools::Itertools;
use std::{
    collections::hash_map::DefaultHasher,
    convert::TryFrom,
    hash::{Hash, Hasher},
    str::FromStr,
};
use uuid::Uuid;

impl ToCasbinString for AccessMethod {
    fn to_casbin_string(self) -> String {
        self.to_string()
    }
}

impl ToCasbinString for &[AccessMethod] {
    /// Converts multiple AccessMethods to a Regex that matches any one of them
    fn to_casbin_string(self) -> String {
        self.iter()
            .map(|&access_method| ToCasbinString::to_casbin_string(access_method))
            .join("|")
    }
}

impl ToCasbinString for PolicyUser {
    fn to_casbin_string(self) -> String {
        format!("user::{}", self.0)
    }
}

impl FromStr for PolicyUser {
    type Err = crate::ParsingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("user::") {
            Ok(PolicyUser(Uuid::from_str(s.trim_start_matches("user::"))?))
        } else {
            Err(crate::ParsingError::PolicyUser(s.to_owned()))
        }
    }
}

impl ToCasbinString for PolicyRole {
    fn to_casbin_string(self) -> String {
        format!("role::{}", self.0)
    }
}

impl FromStr for PolicyRole {
    type Err = crate::ParsingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("role::") {
            Ok(PolicyRole(s.trim_start_matches("role::").to_string()))
        } else {
            Err(crate::ParsingError::PolicyInternalGroup(s.to_owned()))
        }
    }
}

impl ToCasbinString for PolicyGroup {
    fn to_casbin_string(self) -> String {
        format!("group::{}", self.0)
    }
}

impl FromStr for PolicyGroup {
    type Err = crate::ParsingError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("group::") {
            Ok(PolicyGroup(s.trim_start_matches("group::").to_string()))
        } else {
            Err(crate::ParsingError::PolicyOPGroup(s.to_owned()))
        }
    }
}

impl<T: FromStr<Err = crate::ParsingError> + IsSubject> TryFrom<Vec<String>> for Policy<T> {
    type Error = crate::ParsingError;

    fn try_from(value: Vec<String>) -> Result<Self, Self::Error> {
        if value.len() != 3 {
            return Err(crate::ParsingError::Custom(format!(
                "Wrong amount of casbin args: {:?}",
                value
            )));
        }

        Ok(Policy {
            sub: T::from_str(&value[0])?,
            obj: ResourceId::from(value[1].as_str()),
            act: value[2]
                .split('|')
                .map(AccessMethod::from_str)
                .collect::<Result<Vec<AccessMethod>, _>>()?,
        })
    }
}

impl<S: IsSubject + ToCasbinString> ToCasbin for Policy<S> {
    fn to_casbin_policy(self) -> Vec<String> {
        vec![
            self.sub.to_casbin_string(),
            self.obj.into_inner(),
            self.act.to_casbin_string(),
        ]
    }
}

impl<S: IsSubject + ToCasbinString + Clone> From<Policies<'_, S>> for Vec<Vec<String>> {
    fn from(val: Policies<'_, S>) -> Self {
        Into::<Vec<Policy<S>>>::into(val)
            .into_iter()
            .map(Into::<Policy<S>>::into)
            .map(|policy| policy.to_casbin_policy())
            .collect()
    }
}

impl<S: IsSubject + ToCasbinString + Clone> ToCasbinMultiple for Policies<'_, S> {
    fn to_casbin_policies(self) -> Vec<Vec<String>> {
        self.into()
    }
}

impl EnforceArgs for UserPolicy {
    fn try_into_vec(self) -> Result<Vec<Dynamic>, casbin::Error> {
        Ok(vec![
            Dynamic::from(self.sub.to_casbin_string()),
            Dynamic::from(self.obj.into_inner()),
            Dynamic::from(self.act.to_casbin_string()),
        ])
    }

    fn cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl ToCasbin for GroupToRole {
    fn to_casbin_policy(self) -> Vec<String> {
        vec![self.0.to_casbin_string(), self.1.to_casbin_string()]
    }
}

impl ToCasbin for UserToGroup {
    fn to_casbin_policy(self) -> Vec<String> {
        vec![self.0.to_casbin_string(), self.1.to_casbin_string()]
    }
}

impl ToCasbin for UserToRole {
    fn to_casbin_policy(self) -> Vec<String> {
        vec![self.0.to_casbin_string(), self.1.to_casbin_string()]
    }
}

impl ToCasbinMultiple for PoliciesBuilder<Finished> {
    fn to_casbin_policies(self) -> Vec<Vec<String>> {
        self.user_policies
            .into_iter()
            .map(ToCasbin::to_casbin_policy)
            .chain(
                self.group_policies
                    .into_iter()
                    .map(ToCasbin::to_casbin_policy),
            )
            .chain(
                self.role_policies
                    .into_iter()
                    .map(ToCasbin::to_casbin_policy),
            )
            .collect()
    }
}
