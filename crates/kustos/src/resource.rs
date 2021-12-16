//! Abstract resource authz types.
//!
//! Kustos limits supported resources to be identified by a valid URL path.
//! As a resource can be identified with a URL in general we talk here about reduced URI which consists only of the path part.
//! Reason is that for different authorities you can have different permissions. Thus in the context of authz a resource without the authority part makes more sense.
//! It is all local to this deployment. Furthermore, we enforce scheme independent thus this is also not part of our reduced URI.
//! The URIs used with Kustos are currently limited to the following format `/{resourceName}/{resourceId}` where resourceName is static for all instances of that particular resource.
//! Supporting relative resources (e.g. entities with a primary key consisting of a multiple foreign keys) are an open issue currently.
//!
//! All resources need to implement the [`Resource`] trait
use crate::{error::ResourceParseError, policy::Policy, subject::IsSubject};
use std::{fmt::Display, ops::Deref, str::FromStr};

/// This trait is used to allow the retrieval of resource reduced URL prefixes as well as retrieving
/// the reduced URL
pub trait Resource: Sized + Display + FromStr<Err = ResourceParseError> {
    /// URI prefix of the ID of this resource
    ///
    /// # Example
    ///
    /// * `/rooms/`
    /// * `/users/`
    const PREFIX: &'static str;

    /// Returns path part of the URL to access this specific resource
    fn resource_id(&self) -> ResourceId {
        // Assert correct usage of this trait in debug builds only.
        debug_assert!(Self::PREFIX.starts_with('/') && Self::PREFIX.ends_with('/'));

        ResourceId(format!("{}{}", Self::PREFIX, self))
    }
}

/// Represents a accessible resource
///
/// Use this to represent the URL without scheme and authority to represent the respective resource.
///
/// # Example
///
/// * `/users/1` to represent the resource of user with id = 1
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ResourceId(pub(crate) String);
impl<T: IsSubject> From<Policy<T>> for ResourceId {
    fn from(policy: Policy<T>) -> Self {
        policy.obj
    }
}

impl ResourceId {
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn with_suffix(&self, suffix: &str) -> ResourceId {
        let mut inner = self.0.clone();
        inner.push_str(suffix);
        ResourceId(inner)
    }
}

impl AsRef<str> for ResourceId {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Deref for ResourceId {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&str> for ResourceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ResourceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}
/// Response from fetching all implicit accessible resources.
///
/// If a subject has access to a wildcard `/*` or `/resourceName/*` [`AccessibleResources::All`]
/// should be returned, else a List of all accessible resources via [`AccessibleResources::List`]
pub enum AccessibleResources<T: Resource + FromStr> {
    List(Vec<T>),
    All,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ResourceX(uuid::Uuid);

    impl Display for ResourceX {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.to_hyphenated().fmt(f)
        }
    }

    impl Resource for ResourceX {
        const PREFIX: &'static str = "/resources/";
    }

    impl FromStr for ResourceX {
        type Err = ResourceParseError;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            s.parse().map(Self).map_err(ResourceParseError::Uuid)
        }
    }

    #[test]
    fn test_a() {
        let x = ResourceId("/resources/00000000-0000-0000-0000-000000000000".to_string());

        let target: ResourceX = x.strip_prefix(ResourceX::PREFIX).unwrap().parse().unwrap();
        assert_eq!(target.0, uuid::Uuid::nil())
    }
}
