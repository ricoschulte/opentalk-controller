//! Casbin-rs adapter taken from <https://github.com/casbin-rs/diesel-adapter> but inlined to allow for our migration
use super::block;
use crate::db::{
    self,
    casbin::{CasbinRule, NewCasbinRule},
};
use async_trait::async_trait;
use casbin::{error::AdapterError, Adapter, Error as CasbinError, Filter, Model, Result};
use database::Db;
use std::sync::Arc;

impl From<crate::Error> for CasbinError {
    fn from(e: crate::Error) -> Self {
        CasbinError::AdapterError(AdapterError(Box::new(e)))
    }
}

pub struct CasbinAdapter {
    db: Arc<Db>,
    is_filtered: bool,
}

impl CasbinAdapter {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            is_filtered: false,
        }
    }
}

#[async_trait]
impl Adapter for CasbinAdapter {
    #[tracing::instrument(level = "trace", skip(self, m))]
    async fn load_policy(&self, m: &mut dyn Model) -> Result<()> {
        let db = self.db.clone();

        let rules = block(move || {
            let mut conn = db.get_conn()?;

            db::load_policy(&mut conn)
        })
        .await
        .map_err(|e| AdapterError(Box::new(e)))?
        .map_err(|e| AdapterError(Box::new(e)))?;

        for casbin_rule in &rules {
            let rule = load_policy_line(casbin_rule);

            if let Some(ref sec) = casbin_rule.ptype.chars().next().map(|x| x.to_string()) {
                if let Some(ast_map) = m.get_mut_model().get_mut(sec) {
                    if let Some(ast) = ast_map.get_mut(&casbin_rule.ptype) {
                        if let Some(rule) = rule {
                            ast.policy.insert(rule);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn clear_policy(&mut self) -> Result<()> {
        let db = self.db.clone();

        block(move || {
            let mut conn = db.get_conn()?;

            db::clear_policy(&mut conn)
        })
        .await
        .map_err(|e| AdapterError(Box::new(e)))?
        .map_err(|e| AdapterError(Box::new(e)))
        .map_err(Into::into)
    }

    #[tracing::instrument(level = "trace", skip(self, m, f), fields(filter_p = ?f.p, filter_g = ?f.g))]
    async fn load_filtered_policy<'a>(&mut self, m: &mut dyn Model, f: Filter<'a>) -> Result<()> {
        let db = self.db.clone();

        let rules = block(move || {
            let mut conn = db.get_conn()?;

            db::load_policy(&mut conn)
        })
        .await
        .map_err(|e| AdapterError(Box::new(e)))?
        .map_err(|e| AdapterError(Box::new(e)))?;

        for casbin_rule in &rules {
            let rule = load_filtered_policy_row(casbin_rule, &f);

            if let Some((is_filtered, rule)) = rule {
                if !is_filtered {
                    if let Some(ref sec) = casbin_rule.ptype.chars().next().map(|x| x.to_string()) {
                        if let Some(ast_map) = m.get_mut_model().get_mut(sec) {
                            if let Some(ast) = ast_map.get_mut(&casbin_rule.ptype) {
                                ast.get_mut_policy().insert(rule);
                            }
                        }
                    }
                } else {
                    self.is_filtered = true;
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self, m))]
    async fn save_policy(&mut self, m: &mut dyn Model) -> Result<()> {
        let db = self.db.clone();

        let mut rules = vec![];

        if let Some(ast_map) = m.get_model().get("p") {
            for (ptype, ast) in ast_map {
                let new_rules = ast
                    .get_policy()
                    .into_iter()
                    .filter_map(|x: &Vec<String>| save_policy_line(ptype, x));

                rules.extend(new_rules);
            }
        }

        if let Some(ast_map) = m.get_model().get("g") {
            for (ptype, ast) in ast_map {
                let new_rules = ast
                    .get_policy()
                    .into_iter()
                    .filter_map(|x: &Vec<String>| save_policy_line(ptype, x));

                rules.extend(new_rules);
            }
        }

        block(move || {
            let mut conn = db.get_conn()?;

            db::save_policy(&mut conn, rules)
        })
        .await
        .map_err(|e| AdapterError(Box::new(e)))?
        .map_err(|e| AdapterError(Box::new(e)))
        .map_err(Into::into)
    }

    /// Adds a policy
    ///
    /// Ignore the boolean, which is set to true in all cases.
    // The go interface for adapters only returns an error,
    // there is not much sense, as you either get an error or a success.
    #[tracing::instrument(level = "trace", skip(self, rule))]
    async fn add_policy(&mut self, _sec: &str, ptype: &str, rule: Vec<String>) -> Result<bool> {
        let db = self.db.clone();
        let ptype_c = ptype.to_string();

        block(move || -> Result<bool> {
            let mut conn = db.get_conn().map_err(|e| AdapterError(Box::new(e)))?;

            match save_policy_line(&ptype_c, &rule) {
                Some(new_rule) => {
                    db::add_policy(&mut conn, new_rule).map_err(|e| AdapterError(Box::new(e)))?;
                    Ok(true)
                }
                None => Err(crate::Error::Custom("Invalid policy".to_string()).into()),
            }
        })
        .await?
        .map_err(|e| AdapterError(Box::new(e)))
        .map_err(Into::into)
    }

    /// Adds multiple policies
    ///
    /// Ignore the boolean, which is set to true in all cases.
    // The go interface for adapters only returns an error,
    // there is not much sense, as you either get an error or a success.
    #[tracing::instrument(level = "trace", skip(self, rules))]
    async fn add_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> Result<bool> {
        let db = self.db.clone();
        let ptype_c = ptype.to_string();

        block(move || -> Result<bool> {
            let mut conn = db.get_conn().map_err(|e| AdapterError(Box::new(e)))?;

            let new_rules = rules
                .iter()
                .filter_map(|x: &Vec<String>| save_policy_line(&ptype_c, x))
                .collect::<Vec<NewCasbinRule>>();

            db::add_policies(&mut conn, new_rules).map_err(|e| AdapterError(Box::new(e)))?;

            Ok(true)
        })
        .await?
        .map_err(|e| AdapterError(Box::new(e)))
        .map_err(Into::into)
    }

    #[tracing::instrument(level = "trace", skip(self, rule))]
    async fn remove_policy(&mut self, _sec: &str, pt: &str, rule: Vec<String>) -> Result<bool> {
        let db = self.db.clone();
        let ptype_c = pt.to_string();

        block(move || {
            let mut conn = db.get_conn()?;

            db::remove_policy(&mut conn, &ptype_c, rule)
        })
        .await
        .map_err(|e| AdapterError(Box::new(e)))?
        .map_err(|e| AdapterError(Box::new(e)))
        .map_err(Into::into)
    }

    #[tracing::instrument(level = "trace", skip(self, rules))]
    async fn remove_policies(
        &mut self,
        _sec: &str,
        pt: &str,
        rules: Vec<Vec<String>>,
    ) -> Result<bool> {
        let db = self.db.clone();
        let ptype_c = pt.to_string();

        block(move || {
            let mut conn = db.get_conn()?;

            db::remove_policies(&mut conn, &ptype_c, rules)
        })
        .await
        .map_err(|e| AdapterError(Box::new(e)))?
        .map_err(|e| AdapterError(Box::new(e)))
        .map_err(Into::into)
    }

    #[tracing::instrument(level = "trace", skip(self, field_index, field_values))]
    async fn remove_filtered_policy(
        &mut self,
        _sec: &str,
        pt: &str,
        field_index: usize,
        field_values: Vec<String>,
    ) -> Result<bool> {
        if field_index <= 5 && !field_values.is_empty() {
            let db = self.db.clone();
            let ptype_c = pt.to_string();

            block(move || {
                let mut conn = db.get_conn()?;

                db::remove_filtered_policy(&mut conn, &ptype_c, field_index, field_values)
            })
            .await
            .map_err(|e| AdapterError(Box::new(e)))?
            .map_err(|e| AdapterError(Box::new(e)))
            .map_err(Into::into)
        } else {
            Ok(false)
        }
    }

    fn is_filtered(&self) -> bool {
        self.is_filtered
    }
}

fn save_policy_line(ptype: &str, rule: &[String]) -> Option<NewCasbinRule> {
    if ptype.trim().is_empty() || rule.is_empty() {
        return None;
    }

    let mut new_rule = NewCasbinRule {
        ptype: ptype.to_owned(),
        v0: String::new(),
        v1: String::new(),
        v2: String::new(),
        v3: String::new(),
        v4: String::new(),
        v5: String::new(),
    };

    new_rule.v0 = rule[0].to_owned();

    if let Some(v1) = rule.get(1) {
        new_rule.v1 = v1.to_owned();
    }

    if let Some(v2) = rule.get(2) {
        new_rule.v2 = v2.into();
    }

    if let Some(v3) = rule.get(3) {
        new_rule.v3 = v3.into();
    }

    if let Some(v4) = rule.get(4) {
        new_rule.v4 = v4.into();
    }

    if let Some(v5) = rule.get(5) {
        new_rule.v5 = v5.into();
    }

    Some(new_rule)
}

fn load_policy_line(casbin_rule: &CasbinRule) -> Option<Vec<String>> {
    if !casbin_rule.ptype.is_empty() {
        return normalize_policy(casbin_rule);
    }

    None
}

fn load_filtered_policy_row(casbin_rule: &CasbinRule, f: &Filter) -> Option<(bool, Vec<String>)> {
    if let Some(sec) = casbin_rule.ptype.chars().next() {
        if let Some(policy) = normalize_policy(casbin_rule) {
            let mut is_filtered = false;
            if sec == 'p' {
                for (i, rule) in f.p.iter().enumerate() {
                    if !rule.is_empty() && rule != &policy[i] {
                        is_filtered = true
                    }
                }
            } else if sec == 'g' {
                for (i, rule) in f.g.iter().enumerate() {
                    if !rule.is_empty() && rule != &policy[i] {
                        is_filtered = true
                    }
                }
            } else {
                return None;
            }
            return Some((is_filtered, policy));
        }
    }

    None
}

// Trims elements from the end that are empty
fn normalize_policy(casbin_rule: &CasbinRule) -> Option<Vec<String>> {
    let mut result = vec![
        &casbin_rule.v0,
        &casbin_rule.v1,
        &casbin_rule.v2,
        &casbin_rule.v3,
        &casbin_rule.v4,
        &casbin_rule.v5,
    ];

    let first_non_empty = result.iter().enumerate().rfind(|(_, v)| !v.is_empty());

    if let Some((i, _)) = first_non_empty {
        result.truncate(i + 1);
        return Some(result.into_iter().map(ToOwned::to_owned).collect());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::{query_helper, Db};
    use diesel::{Connection, PgConnection, RunQueryDsl};
    use serial_test::serial;

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
    m = g(r.sub, p.sub) && r.obj == p.obj && r.act == p.act"#;

    const MODEL2: &str = r#"
    [request_definition]
    r = sub, dom, obj, act
    
    [policy_definition]
    p = sub, dom, obj, act
    
    [role_definition]
    g = _, _, _
    
    [policy_effect]
    e = some(where (p.eft == allow))
    
    [matchers]
    m = g(r.sub, p.sub, r.dom) && r.dom == p.dom && r.obj == p.obj && r.act == p.act"#;

    fn to_owned(v: Vec<&str>) -> Vec<String> {
        v.into_iter().map(|x| x.to_owned()).collect()
    }

    fn to_owned2(v: Vec<Vec<&str>>) -> Vec<Vec<String>> {
        v.into_iter().map(to_owned).collect()
    }

    fn change_database_of_url(database_url: &str, default_database: &str) -> (String, String) {
        let base = ::url::Url::parse(database_url).unwrap();
        let database = base.path_segments().unwrap().last().unwrap().to_owned();
        let mut new_url = base.join(default_database).unwrap();
        new_url.set_query(base.query());
        (database, new_url.into())
    }

    async fn setup() -> anyhow::Result<Arc<Db>> {
        use anyhow::Context;
        let url = std::env::var("KUSTOS_TESTS_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:password123@localhost:5432/k3k".to_string());

        if PgConnection::establish(&url).is_err() {
            let (database, postgres_url) = change_database_of_url(&url, "postgres");
            log::info!("Creating database: {}", database);
            let mut conn = PgConnection::establish(&postgres_url)?;
            query_helper::create_database(&database).execute(&mut conn)?;
        }

        db_storage::migrations::migrate_from_url(&url)
            .await
            .context("Migration failed")?;
        let db =
            Arc::new(Db::connect_url(&url, 10, Some(2)).context("Failed to connect to database")?);
        let db_clone = db.clone();
        block(move || {
            let mut conn = db_clone.get_conn()?;

            db::clear_policy(&mut conn)
        })
        .await
        .unwrap()
        .unwrap();

        Ok(db)
    }

    #[tokio::test]
    #[serial]
    async fn test_adapter() {
        use casbin::prelude::*;
        let memory_adapter = MemoryAdapter::default();

        let m = DefaultModel::from_str(MODEL).await.unwrap();

        let mut e = Enforcer::new(m, memory_adapter).await.unwrap();
        e.add_policies(to_owned2(vec![
            vec!["alice", "data1", "read"],
            vec!["bob", "data2", "write"],
            vec!["data2_admin", "data2", "read"],
            vec!["data2_admin", "data2", "write"],
        ]))
        .await
        .unwrap();
        e.add_grouping_policy(vec!["alice".into(), "data2_admin".into()])
            .await
            .unwrap();
        let db = setup().await.unwrap();
        let mut adapter = { CasbinAdapter::new(db) };

        assert!(adapter.save_policy(e.get_mut_model()).await.is_ok());

        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["alice", "data1", "read"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["bob", "data2", "write"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["data2_admin", "data2", "read"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["data2_admin", "data2", "write"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "g", to_owned(vec!["alice", "data2_admin"]))
            .await
            .is_ok());

        assert!(adapter
            .add_policy("p", "p", to_owned(vec!["alice", "data1", "read"]))
            .await
            .is_ok());
        assert!(adapter
            .add_policy("p", "p", to_owned(vec!["bob", "data2", "write"]))
            .await
            .is_ok());
        assert!(adapter
            .add_policy("p", "p", to_owned(vec!["data2_admin", "data2", "read"]))
            .await
            .is_ok());
        assert!(adapter
            .add_policy("p", "p", to_owned(vec!["data2_admin", "data2", "write"]))
            .await
            .is_ok());

        assert!(adapter
            .remove_policies(
                "p",
                "p",
                vec![
                    to_owned(vec!["alice", "data1", "read"]),
                    to_owned(vec!["bob", "data2", "write"]),
                    to_owned(vec!["data2_admin", "data2", "read"]),
                    to_owned(vec!["data2_admin", "data2", "write"]),
                ]
            )
            .await
            .is_ok());

        assert!(adapter
            .add_policies(
                "p",
                "p",
                vec![
                    to_owned(vec!["alice", "data1", "read"]),
                    to_owned(vec!["bob", "data2", "write"]),
                    to_owned(vec!["data2_admin", "data2", "read"]),
                    to_owned(vec!["data2_admin", "data2", "write"]),
                ]
            )
            .await
            .is_ok());

        assert!(adapter
            .add_policy("g", "g", to_owned(vec!["alice", "data2_admin"]))
            .await
            .is_ok());

        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["alice", "data1", "read"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["bob", "data2", "write"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["data2_admin", "data2", "read"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("p", "p", to_owned(vec!["data2_admin", "data2", "write"]))
            .await
            .is_ok());
        assert!(adapter
            .remove_policy("g", "g", to_owned(vec!["alice", "data2_admin"]))
            .await
            .is_ok());

        assert!(!adapter
            .remove_policy(
                "g",
                "g",
                to_owned(vec!["alice", "data2_admin", "not_exists"])
            )
            .await
            .unwrap());

        assert!(adapter
            .add_policy("g", "g", to_owned(vec!["alice", "data2_admin"]))
            .await
            .is_ok());
        assert!(adapter
            .add_policy("g", "g", to_owned(vec!["alice", "data2_admin"]))
            .await
            .is_err());

        assert!(!adapter
            .remove_filtered_policy(
                "g",
                "g",
                0,
                to_owned(vec!["alice", "data2_admin", "not_exists"]),
            )
            .await
            .unwrap());

        assert!(adapter
            .remove_filtered_policy("g", "g", 0, to_owned(vec!["alice", "data2_admin"]))
            .await
            .is_ok());

        assert!(adapter
            .add_policy(
                "g",
                "g",
                to_owned(vec!["alice", "data2_admin", "domain1", "domain2"]),
            )
            .await
            .is_ok());
        assert!(adapter
            .remove_filtered_policy(
                "g",
                "g",
                1,
                to_owned(vec!["data2_admin", "domain1", "domain2"]),
            )
            .await
            .is_ok());

        // shadow the previous enforcer
        let m = DefaultModel::from_str(MODEL2).await.unwrap();
        let a = MemoryAdapter::default();

        let mut e = Enforcer::new(m, a).await.unwrap();
        e.add_policies(to_owned2(vec![
            vec!["admin", "domain1", "data1", "read"],
            vec!["admin", "domain1", "data1", "write"],
            vec!["admin", "domain2", "data2", "read"],
            vec!["admin", "domain2", "data2", "write"],
        ]))
        .await
        .unwrap();
        e.add_grouping_policies(to_owned2(vec![
            vec!["alice", "admin", "domain1"],
            vec!["bob", "admin", "domain2"],
        ]))
        .await
        .unwrap();

        assert!(adapter.save_policy(e.get_mut_model()).await.is_ok());
        e.set_adapter(adapter).await.unwrap();

        let filter = Filter {
            p: vec!["", "domain1"],
            g: vec!["", "", "domain1"],
        };

        e.load_filtered_policy(filter).await.unwrap();

        assert!(e.enforce(("alice", "domain1", "data1", "read")).unwrap());
        assert!(e.enforce(("alice", "domain1", "data1", "write")).unwrap());
        assert!(!e.enforce(("alice", "domain1", "data2", "read")).unwrap());
        assert!(!e.enforce(("alice", "domain1", "data2", "write")).unwrap());
        assert!(!e.enforce(("bob", "domain2", "data2", "read")).unwrap());
        assert!(!e.enforce(("bob", "domain2", "data2", "write")).unwrap());
    }

    #[test]
    fn test_normalize_policy() {
        let casbin_rule = CasbinRule {
            id: 1,
            ptype: "p".to_owned(),
            v0: "".to_owned(),
            v1: "".to_owned(),
            v2: "".to_owned(),
            v3: "".to_owned(),
            v4: "".to_owned(),
            v5: "".to_owned(),
        };
        assert_eq!(normalize_policy(&casbin_rule), None);
        let casbin_rule = CasbinRule {
            id: 1,
            ptype: "p".to_owned(),
            v0: "alice".to_owned(),
            v1: "data".to_owned(),
            v2: "read".to_owned(),
            v3: "".to_owned(),
            v4: "".to_owned(),
            v5: "".to_owned(),
        };
        assert_eq!(
            normalize_policy(&casbin_rule).unwrap(),
            vec!["alice", "data", "read"]
        );
    }
}
