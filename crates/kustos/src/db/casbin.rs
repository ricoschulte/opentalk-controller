use super::schema::casbin_rule::{self, dsl::*};
use crate::eq_empty;
use database::{DatabaseError, DbInterface, Result};
use diesel::{
    result::Error as DieselError, BoolExpressionMethods, ExpressionMethods, QueryDsl, RunQueryDsl,
};

#[derive(Queryable, Identifiable, Debug)]
#[table_name = "casbin_rule"]
pub struct CasbinRule {
    pub id: i32,
    pub ptype: String,
    pub v0: String,
    pub v1: String,
    pub v2: String,
    pub v3: String,
    pub v4: String,
    pub v5: String,
}

#[derive(Insertable, Clone, Debug)]
#[table_name = "casbin_rule"]
pub struct NewCasbinRule {
    pub ptype: String,
    pub v0: String,
    pub v1: String,
    pub v2: String,
    pub v3: String,
    pub v4: String,
    pub v5: String,
}

pub trait DbCasbinEx: DbInterface {
    #[tracing::instrument(skip(self, rule))]
    fn remove_policy(&self, pt: &str, rule: Vec<String>) -> Result<bool> {
        let rule = normalize_casbin_rule(rule, 0);
        let conn = self.get_conn()?;

        let filter = ptype
            .eq(pt)
            .and(v0.eq(&rule[0]))
            .and(v1.eq(&rule[1]))
            .and(v2.eq(&rule[2]))
            .and(v3.eq(&rule[3]))
            .and(v4.eq(&rule[4]))
            .and(v5.eq(&rule[5]));

        diesel::delete(casbin_rule.filter(filter))
            .execute(&conn)
            .map(|n| n == 1)
            .map_err(DatabaseError::from)
    }

    #[tracing::instrument(skip(self, rules))]
    fn remove_policies(&self, pt: &str, rules: Vec<Vec<String>>) -> Result<bool> {
        let conn = self.get_conn()?;

        conn.build_transaction().run::<bool, DatabaseError, _>(|| {
            for rule in rules {
                let rule = normalize_casbin_rule(rule, 0);

                let filter = ptype
                    .eq(pt)
                    .and(v0.eq(&rule[0]))
                    .and(v1.eq(&rule[1]))
                    .and(v2.eq(&rule[2]))
                    .and(v3.eq(&rule[3]))
                    .and(v4.eq(&rule[4]))
                    .and(v5.eq(&rule[5]));

                match diesel::delete(casbin_rule.filter(filter)).execute(&conn) {
                    Ok(n) if n == 1 => continue,
                    _ => return Err(DieselError::RollbackTransaction.into()),
                }
            }

            Ok(true)
        })
    }

    #[tracing::instrument(skip(self, field_index, field_values))]
    fn remove_filtered_policy(
        &self,
        pt: &str,
        field_index: usize,
        field_values: Vec<String>,
    ) -> Result<bool> {
        let conn = self.get_conn()?;
        let field_values = normalize_casbin_rule(field_values, field_index);

        let boxed_query = if field_index == 5 {
            diesel::delete(casbin_rule.filter(ptype.eq(pt).and(eq_empty!(&field_values[0], v5))))
                .into_boxed()
        } else if field_index == 4 {
            diesel::delete(
                casbin_rule.filter(
                    ptype
                        .eq(pt)
                        .and(eq_empty!(&field_values[0], v4))
                        .and(eq_empty!(&field_values[1], v5)),
                ),
            )
            .into_boxed()
        } else if field_index == 3 {
            diesel::delete(
                casbin_rule.filter(
                    ptype
                        .eq(pt)
                        .and(eq_empty!(&field_values[0], v3))
                        .and(eq_empty!(&field_values[1], v4))
                        .and(eq_empty!(&field_values[2], v5)),
                ),
            )
            .into_boxed()
        } else if field_index == 2 {
            diesel::delete(
                casbin_rule.filter(
                    ptype
                        .eq(pt)
                        .and(eq_empty!(&field_values[0], v2))
                        .and(eq_empty!(&field_values[1], v3))
                        .and(eq_empty!(&field_values[2], v4))
                        .and(eq_empty!(&field_values[3], v5)),
                ),
            )
            .into_boxed()
        } else if field_index == 1 {
            diesel::delete(
                casbin_rule.filter(
                    ptype
                        .eq(pt)
                        .and(eq_empty!(&field_values[0], v1))
                        .and(eq_empty!(&field_values[1], v2))
                        .and(eq_empty!(&field_values[2], v3))
                        .and(eq_empty!(&field_values[3], v4))
                        .and(eq_empty!(&field_values[4], v5)),
                ),
            )
            .into_boxed()
        } else {
            diesel::delete(
                casbin_rule.filter(
                    ptype
                        .eq(pt)
                        .and(eq_empty!(&field_values[0], v0))
                        .and(eq_empty!(&field_values[1], v1))
                        .and(eq_empty!(&field_values[2], v2))
                        .and(eq_empty!(&field_values[3], v3))
                        .and(eq_empty!(&field_values[4], v4))
                        .and(eq_empty!(&field_values[5], v5)),
                ),
            )
            .into_boxed()
        };

        boxed_query
            .execute(&conn)
            .map(|n| n >= 1)
            .map_err(DatabaseError::from)
    }

    #[tracing::instrument(skip(self))]
    fn clear_policy(&self) -> Result<()> {
        let conn = self.get_conn()?;

        diesel::delete(casbin_rule)
            .execute(&conn)
            .map(|_| ())
            .map_err(DatabaseError::from)
    }

    #[tracing::instrument(skip(self, rules))]
    fn save_policy(&self, rules: Vec<NewCasbinRule>) -> Result<()> {
        let conn = self.get_conn()?;

        conn.build_transaction().run::<_, DatabaseError, _>(|| {
            if diesel::delete(casbin_rule).execute(&conn).is_err() {
                return Err(DieselError::RollbackTransaction.into());
            }

            diesel::insert_into(casbin_rule)
                .values(&rules)
                .execute(&conn)
                .and_then(|n| {
                    if n == rules.len() {
                        Ok(())
                    } else {
                        Err(DieselError::RollbackTransaction)
                    }
                })
                .map_err(|_| DieselError::RollbackTransaction.into())
        })
    }

    #[tracing::instrument(skip(self))]
    fn load_policy(&self) -> Result<Vec<CasbinRule>> {
        let conn = self.get_conn()?;

        casbin_rule
            .load::<CasbinRule>(&conn)
            .map_err(DatabaseError::from)
    }

    #[tracing::instrument(skip(self, new_rule))]
    fn add_policy(&self, new_rule: NewCasbinRule) -> Result<bool> {
        let conn = self.get_conn()?;

        diesel::insert_into(casbin_rule)
            .values(&new_rule)
            .execute(&conn)
            .map(|n| n == 1)
            .map_err(DatabaseError::from)
    }

    #[tracing::instrument(skip(self, new_rules))]
    fn add_policies(&self, new_rules: Vec<NewCasbinRule>) -> Result<bool> {
        let conn = self.get_conn()?;

        conn.build_transaction().run::<_, DatabaseError, _>(|| {
            diesel::insert_into(casbin_rule)
                .values(&new_rules)
                .execute(&*conn)
                .and_then(|n| {
                    if n == new_rules.len() {
                        Ok(true)
                    } else {
                        Err(DieselError::RollbackTransaction)
                    }
                })
                .map_err(|_| DieselError::RollbackTransaction.into())
        })
    }
}

fn normalize_casbin_rule(mut rule: Vec<String>, field_index: usize) -> Vec<String> {
    rule.resize(6 - field_index, String::new());
    rule
}

impl<T: DbInterface> DbCasbinEx for T {}
