use super::users::PublicUserProfile;
use controller_shared::settings::Settings;
use database::DbConnection;
use database::Result;
use db_storage::users::{User, UserId};
use db_storage::utils::HasUsers;
use serde::de;
use serde::de::Visitor;
use serde::Deserialize;
use serde::Deserializer;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

/// Utility to fetch user profiles batched
///
/// See [`db_storage::utils::HasUsers`]
#[derive(Default)]
pub struct GetUserProfilesBatched {
    users: Vec<UserId>,
}

impl GetUserProfilesBatched {
    pub fn new() -> Self {
        Self { users: vec![] }
    }

    pub fn add(&mut self, has_users: impl HasUsers) -> &mut Self {
        has_users.populate(&mut self.users);
        self
    }

    pub fn fetch(
        &mut self,
        settings: &Settings,
        conn: &mut DbConnection,
    ) -> Result<UserProfilesBatch> {
        if self.users.is_empty() {
            return Ok(UserProfilesBatch {
                users: HashMap::new(),
            });
        }

        self.users.sort_unstable();
        self.users.dedup();

        User::get_all_by_ids(conn, &self.users)
            .map(|users| {
                users
                    .into_iter()
                    .map(|user| (user.id, PublicUserProfile::from_db(settings, user)))
                    .collect()
            })
            .map(|users| UserProfilesBatch { users })
    }
}

pub struct UserProfilesBatch {
    users: HashMap<UserId, PublicUserProfile>,
}

impl UserProfilesBatch {
    pub fn get(&self, id: UserId) -> PublicUserProfile {
        self.users
            .get(&id)
            .expect("tried to get user-profile without fetching it first")
            .clone()
    }
}

/// Helper function to deserialize Option<Option<T>>
/// https://github.com/serde-rs/serde/issues/984
pub(super) fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod test {
    use super::*;
    use pretty_assertions::assert_eq;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Test {
        #[serde(default, deserialize_with = "deserialize_some")]
        test: Option<Option<String>>,
    }

    #[test]
    fn deserialize_option_option() {
        let none = "{}";
        let some_none = r#"{"test":null}"#;
        let some_some = r#"{"test":"test"}"#;

        assert_eq!(
            serde_json::from_str::<Test>(none).unwrap(),
            Test { test: None }
        );
        assert_eq!(
            serde_json::from_str::<Test>(some_none).unwrap(),
            Test { test: Some(None) }
        );
        assert_eq!(
            serde_json::from_str::<Test>(some_some).unwrap(),
            Test {
                test: Some(Some("test".into()))
            }
        );
    }
}

pub fn comma_separated<'de, V, T, D>(deserializer: D) -> Result<V, D::Error>
where
    V: FromIterator<T>,
    T: FromStr,
    T::Err: fmt::Display,
    D: Deserializer<'de>,
{
    struct CommaSeparated<V, T>(PhantomData<(T, V)>);

    impl<'de, V, T> Visitor<'de> for CommaSeparated<V, T>
    where
        V: FromIterator<T>,
        T: FromStr,
        T::Err: fmt::Display,
    {
        type Value = V;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string containing comma-separated elements")
        }

        fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let iter = s.split(',').map(FromStr::from_str);
            iter.collect::<Result<_, _>>().map_err(de::Error::custom)
        }
    }

    let visitor = CommaSeparated(PhantomData);
    deserializer.deserialize_str(visitor)
}
