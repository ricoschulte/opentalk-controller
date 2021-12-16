//! Casbin bindings for the public API
use casbin::DefaultModel;

use crate::error::Error;

pub(crate) mod diesel_adapter;
pub(crate) mod impls;
pub(crate) mod rbac_api_ex;
pub(crate) mod synced_enforcer;

pub trait ToCasbin {
    fn to_casbin_policy(self) -> Vec<String>;
}

//TODO(r.floren) finr better name for this. We do not want that to be part of ToCasbin as then we need to impl to_casbin_policy ofr UserPolicies<'_>
pub trait ToCasbinMultiple {
    fn to_casbin_policies(self) -> Vec<Vec<String>>;
}

/// This trait is used to allow different struct to be used with casbin.
///
/// This allows strict rules for keys in the permissions system.
/// Similar to redis we use prefixes here to differentiate resources.
/// E.g. user::<UUID> and group::<ID>
pub trait ToCasbinString {
    fn to_casbin_string(self) -> String;
}

/// Default Model
///
/// Matches sub, obj, act with:
/// sub group membership g(r.sub, p.sub)
/// obj keyMatch3 (URL template spec matching with {placeholder} and * ), placeholder with the same id matches the same string.
/// act regexMatch (policy regex matches request)
///
/// OPTIONS calls are generally allowed
/// subjects in the role::administrator g role are allowed.
const MODEL: &str = r#"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.act == "OPTIONS" || g(r.sub, p.sub) && keyMatch3(r.obj, p.obj) && regexMatch(r.act, p.act)
"#;
pub(crate) async fn default_acl_model() -> DefaultModel {
    DefaultModel::from_str(MODEL).await.unwrap()
}

/// Internal use only, copy from controller
pub(crate) async fn block<F, R>(f: F) -> std::result::Result<R, Error>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();

    let fut = tokio::task::spawn_blocking(move || span.in_scope(f));

    fut.await.map_err(Error::BlockingError)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::access::AccessMethod;
    use crate::internal::rbac_api_ex::RbacApiEx;
    use crate::subject::PolicyUser;
    use crate::{UserPolicies, UserPolicy};
    use casbin::{CoreApi, MgmtApi};
    use std::convert::TryInto;
    use std::iter::FromIterator;
    use std::str::FromStr;
    use std::vec;
    use uuid::Uuid;

    fn to_owned(v: Vec<&str>) -> Vec<String> {
        v.into_iter().map(|x| x.to_owned()).collect()
    }

    fn to_owned2(v: Vec<Vec<&str>>) -> Vec<Vec<String>> {
        v.into_iter().map(to_owned).collect()
    }

    #[test]
    fn test_user_policy() {
        let policy = UserPolicy::new(PolicyUser(uuid::Uuid::nil()), "/test", AccessMethod::Read);
        assert_eq!(
            policy.to_casbin_policy(),
            vec![
                format!("user::{}", uuid::Uuid::nil()),
                "/test".to_string(),
                "read".to_string()
            ]
        )
    }

    #[test]
    fn test_user_polices() {
        let policies: Vec<Vec<String>> = UserPolicies::new(
            &PolicyUser(uuid::Uuid::nil()),
            &[(
                &"/data/test".into(),
                &[AccessMethod::Read, AccessMethod::Write],
            )],
        )
        .into();

        assert_eq!(
            policies,
            vec![vec![
                format!("user::{}", uuid::Uuid::nil()),
                "/data/test".to_string(),
                "read|write".to_string()
            ],]
        );

        let policies: Vec<Vec<String>> = UserPolicies::new(
            &PolicyUser(uuid::Uuid::nil()),
            &[
                (
                    &"/data/test".into(),
                    &[AccessMethod::Read, AccessMethod::Write],
                ),
                (&"/data/test2".into(), &[AccessMethod::Read]),
                (&"/data/test3".into(), &[AccessMethod::Read]),
            ],
        )
        .into();

        assert_eq!(
            policies,
            vec![
                vec![
                    format!("user::{}", uuid::Uuid::nil()),
                    "/data/test".to_string(),
                    "read|write".to_string()
                ],
                vec![
                    format!("user::{}", uuid::Uuid::nil()),
                    "/data/test2".to_string(),
                    "read".to_string()
                ],
                vec![
                    format!("user::{}", uuid::Uuid::nil()),
                    "/data/test3".to_string(),
                    "read".to_string()
                ],
            ]
        );
    }
    #[test]
    fn test_try_from_impls() {
        // Valid input
        let input = vec![
            vec![
                "user::00000000-0000-0000-0000-000000000000".to_string(),
                "/room/00000000-0000-0000-0000-000000000000".to_string(),
                "GET|PUT".to_string(),
            ],
            vec![
                "user::00000000-0000-0000-0000-000000000001".to_string(),
                "/room/00000000-0000-0000-0000-000000000001".to_string(),
                "GET".to_string(),
            ],
        ];
        let should = vec![
            UserPolicy::new(
                PolicyUser(Uuid::nil()),
                "/room/00000000-0000-0000-0000-000000000000",
                [AccessMethod::Get, AccessMethod::Put],
            ),
            UserPolicy::new(
                PolicyUser(Uuid::from_u128(1)),
                "/room/00000000-0000-0000-0000-000000000001",
                AccessMethod::Get,
            ),
        ];
        let is = input
            .into_iter()
            .map(TryInto::<UserPolicy>::try_into)
            .collect::<std::result::Result<Vec<_>, crate::ParsingError>>()
            .unwrap();
        assert_eq!(is, should);

        // Invalid input
        let input = vec![vec![
            "usedr::00000000-0000-0000-0000-000000000000".to_string(),
            "/room/00000000-0000-0000-0000-000000000000".to_string(),
            "GET|PUT".to_string(),
        ]];

        assert!(input
            .into_iter()
            .map(TryInto::<UserPolicy>::try_into)
            .collect::<std::result::Result<Vec<_>, crate::ParsingError>>()
            .is_err());
    }

    #[test]
    fn test_from_str_impls() {
        // Valid input
        let input = "user::00000000-0000-0000-0000-000000000000";
        let is = PolicyUser::from_str(input).unwrap();
        assert_eq!(is, PolicyUser(Uuid::nil()));

        // Invalid input
        let input = "nutzer::00000000-0000-0000-0000-000000000000";
        let is = PolicyUser::from_str(input);
        assert!(is.is_err());
    }

    #[tokio::test]
    async fn test_kustos_types_impl() {
        let m = default_acl_model().await;
        let a = casbin::MemoryAdapter::default();
        let mut enforcer = casbin::Enforcer::new(m, a).await.unwrap();
        enforcer
            .add_policies(to_owned2(vec![
                vec!["role::administrator", "/*", "GET|POST|PUT|DELETE"],
                vec!["role::user", "/rooms", "POST"],
            ]))
            .await
            .unwrap();
        enforcer
            .add_named_grouping_policies(
                "g",
                to_owned2(vec![
                    vec!["user::3079670b-2bad-4023-bf16-95993f462530", "role::user"],
                    vec![
                        "user::00000000-0000-0000-0000-000000000000",
                        "group::/OpenTalk_Administrator",
                    ],
                    vec!["group::/OpenTalk_Administrator", "role::administrator"],
                ]),
            )
            .await
            .unwrap();

        let test = std::collections::HashSet::<String>::from_iter(
            enforcer
                .get_implicit_users_for_role("role::user", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            std::collections::HashSet::from_iter(
                vec!["user::3079670b-2bad-4023-bf16-95993f462530".to_string()].into_iter()
            )
        );

        let test = std::collections::HashSet::<Vec<String>>::from_iter(
            enforcer
                .get_implicit_resources_for_user("user::3079670b-2bad-4023-bf16-95993f462530", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            std::collections::HashSet::from_iter(
                vec![to_owned(vec![
                    "user::3079670b-2bad-4023-bf16-95993f462530",
                    "/rooms",
                    "POST"
                ])]
                .into_iter()
            )
        );
        // User 307 has /rooms POST access via its role::user group
        assert!(enforcer
            .enforce(to_owned(vec![
                "user::3079670b-2bad-4023-bf16-95993f462530",
                "/rooms",
                "POST"
            ]))
            .unwrap());
        // User 000 has /rooms POST access via its group::/OpenTalk_Administrator group via role::user group
        assert!(enforcer
            .enforce(to_owned(vec![
                "user::00000000-0000-0000-0000-000000000000",
                "/rooms/dasdasd",
                "DELETE"
            ]))
            .unwrap());
    }
}
