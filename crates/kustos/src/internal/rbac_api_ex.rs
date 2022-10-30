// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Expands the RbacApi of casbin of some missing functions that are present in casbin go implementation
use casbin::RbacApi;
use std::collections::HashSet;

pub trait RbacApiEx: RbacApi {
    /// Gets implicit users for a role
    #[tracing::instrument(level = "trace", skip(self))]
    fn get_implicit_users_for_role(&mut self, role: &str, domain: Option<&str>) -> Vec<String> {
        let mut res: HashSet<String> = HashSet::new();

        for rm in self.get_role_managers() {
            let users = rm.read().get_users(role, domain);

            res.extend(users);
        }

        res.into_iter().collect()
    }

    /// Gets implicit resources for a user
    #[tracing::instrument(level = "trace", skip(self, user))]
    fn get_implicit_resources_for_user(
        &mut self,
        user: &str,
        domain: Option<&str>,
    ) -> Vec<Vec<String>> {
        let permissions = self.get_implicit_permissions_for_user(user, domain);
        let mut result = Vec::new();
        for permission in permissions {
            // This resource is directly accessible by the user.
            if permission[0] == user {
                result.push(permission.clone());
                continue;
            }

            // Now handle the implicit permissions
            // First collect all rules that are implicitly accessible (v0) by the role (v1)
            // The result is a Vec of Vecs which contain all v0 entries that are accessible by v1.
            // The target v1 can be in v1 to v5 of the direct permissions
            let t = permission
                .iter()
                .skip(1)
                .map(|token| {
                    let mut tokens = self.get_implicit_users_for_role(token, domain);
                    tokens.push(token.clone());
                    tokens
                })
                .collect::<Vec<_>>();

            // Extend each rule in result_local.
            let mut result_local = vec![vec![user.to_string()]];
            let tokens_length = permission.len();
            for i in 0..tokens_length - 1 {
                let mut n = Vec::new();
                for tokens in &t[i] {
                    for policy in &result_local {
                        let mut t = policy.clone();
                        t.push(tokens.clone());
                        n.push(t);
                    }
                }
                result_local = n;
            }
            result.extend(result_local.into_iter());
        }
        result
    }
}
impl<T: RbacApi> RbacApiEx for T {}

#[cfg(test)]
mod test {
    use super::*;
    use casbin::{CoreApi, DefaultModel, Enforcer, MemoryAdapter, MgmtApi};
    use pretty_assertions::assert_eq;
    use std::iter::FromIterator;

    fn to_owned(v: Vec<&str>) -> Vec<String> {
        v.into_iter().map(|x| x.to_owned()).collect()
    }

    fn to_owned2(v: Vec<Vec<&str>>) -> Vec<Vec<String>> {
        v.into_iter().map(to_owned).collect()
    }
    const MODEL: &str = r#"[request_definition]
    r = sub, obj, act

    [policy_definition]
    p = sub, obj, act

    [role_definition]
    g = _, _
    g2 = _, _

    [policy_effect]
    e = some(where (p.eft == allow))

    [matchers]
    m = g(r.sub, p.sub) && g2(r.obj, p.obj) && regexMatch(r.act, p.act)"#;

    #[tokio::test]
    async fn test_get_implicit_users_for_role() {
        let m = DefaultModel::from_str(MODEL).await.unwrap();
        let a = MemoryAdapter::default();
        let mut enforcer = Enforcer::new(m, a).await.unwrap();
        enforcer
            .add_policies(to_owned2(vec![
                vec!["alice", "/pen/1", "read"],
                vec!["alice", "/pen/2", "read"],
                vec!["book_admin", "book_group", "read"],
                vec!["pen_admin", "pen_group", "read"],
            ]))
            .await
            .unwrap();
        enforcer
            .add_named_grouping_policies(
                "g",
                to_owned2(vec![
                    vec!["alice", "book_admin"],
                    vec!["bob", "pen_admin"],
                    vec!["cathy", "/book/1/2/3/4/5"],
                    vec!["cathy", "pen_admin"],
                ]),
            )
            .await
            .unwrap();
        enforcer
            .add_named_grouping_policies(
                "g2",
                to_owned2(vec![
                    vec!["/book/*", "book_group"],
                    vec!["/book/:id", "book_group"],
                    vec!["/pen/:id", "pen_group"],
                    vec!["/book2/{id}", "book_group"],
                    vec!["/pen2/{id}", "pen_group"],
                ]),
            )
            .await
            .unwrap();
        let test = HashSet::<String>::from_iter(
            enforcer
                .get_implicit_users_for_role("book_admin", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(vec!["alice".to_string()].into_iter())
        );
        let test = HashSet::<String>::from_iter(
            enforcer
                .get_implicit_users_for_role("pen_admin", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(vec!["cathy".to_string(), "bob".to_string()].into_iter())
        );
        let test = HashSet::<String>::from_iter(
            enforcer
                .get_implicit_users_for_role("book_group", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(
                vec![
                    "/book/*".to_string(),
                    "/book2/{id}".to_string(),
                    "/book/:id".to_string()
                ]
                .into_iter()
            )
        );
        let test = HashSet::<String>::from_iter(
            enforcer
                .get_implicit_users_for_role("pen_group", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(vec!["/pen/:id".to_string(), "/pen2/{id}".to_string()].into_iter())
        );
    }
    #[tokio::test]
    async fn test_get_implicit_resources_for_user() {
        let m = DefaultModel::from_str(MODEL).await.unwrap();
        let a = MemoryAdapter::default();
        let mut enforcer = Enforcer::new(m, a).await.unwrap();
        enforcer
            .add_policies(to_owned2(vec![
                vec!["alice", "/pen/1", "GET"],
                vec!["alice", "/pen2/1", "GET"],
                vec!["book_admin", "book_group", "GET"],
                vec!["pen_admin", "pen_group", "GET"],
            ]))
            .await
            .unwrap();
        enforcer
            .add_named_grouping_policies(
                "g",
                to_owned2(vec![
                    vec!["alice", "book_admin"],
                    vec!["bob", "pen_admin"],
                    vec!["cathy", "/book/1/2/3/4/5"],
                    vec!["cathy", "pen_admin"],
                ]),
            )
            .await
            .unwrap();
        enforcer
            .add_named_grouping_policies(
                "g2",
                to_owned2(vec![
                    vec!["/book/*", "book_group"],
                    vec!["/book/:id", "book_group"],
                    vec!["/pen/:id", "pen_group"],
                    vec!["/book2/{id}", "book_group"],
                    vec!["/pen2/{id}", "pen_group"],
                ]),
            )
            .await
            .unwrap();
        let test = HashSet::<Vec<String>>::from_iter(
            enforcer
                .get_implicit_resources_for_user("alice", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(
                vec![
                    to_owned(vec!["alice", "/pen/1", "GET"]),
                    to_owned(vec!["alice", "/pen2/1", "GET"]),
                    to_owned(vec!["alice", "/book/:id", "GET"]),
                    to_owned(vec!["alice", "/book2/{id}", "GET"]),
                    to_owned(vec!["alice", "/book/*", "GET"]),
                    to_owned(vec!["alice", "book_group", "GET"])
                ]
                .into_iter()
            )
        );
        let test = HashSet::<Vec<String>>::from_iter(
            enforcer
                .get_implicit_resources_for_user("bob", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(
                vec![
                    to_owned(vec!["bob", "pen_group", "GET"]),
                    to_owned(vec!["bob", "/pen/:id", "GET"]),
                    to_owned(vec!["bob", "/pen2/{id}", "GET"])
                ]
                .into_iter()
            )
        );
        let test = HashSet::<Vec<String>>::from_iter(
            enforcer
                .get_implicit_resources_for_user("cathy", None)
                .into_iter(),
        );
        assert_eq!(
            test,
            HashSet::from_iter(
                vec![
                    to_owned(vec!["cathy", "pen_group", "GET"]),
                    to_owned(vec!["cathy", "/pen/:id", "GET"]),
                    to_owned(vec!["cathy", "/pen2/{id}", "GET"])
                ]
                .into_iter()
            )
        );
    }
}
