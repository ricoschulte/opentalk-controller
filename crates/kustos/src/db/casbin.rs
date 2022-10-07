#![allow(clippy::extra_unused_lifetimes)]

use super::schema::casbin_rule::{self, dsl::*};
use crate::eq_empty;
use database::{DatabaseError, DbConnection, Result};
use diesel::result::Error as DieselError;
use diesel::{BoolExpressionMethods, Connection, ExpressionMethods, QueryDsl, RunQueryDsl};

#[derive(Queryable, Identifiable, Debug)]
#[diesel(table_name = casbin_rule)]
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
#[diesel(table_name = casbin_rule)]
pub struct NewCasbinRule {
    pub ptype: String,
    pub v0: String,
    pub v1: String,
    pub v2: String,
    pub v3: String,
    pub v4: String,
    pub v5: String,
}

#[tracing::instrument(err, skip_all)]
pub fn remove_policy(conn: &mut DbConnection, pt: &str, rule: Vec<String>) -> Result<bool> {
    let rule = normalize_casbin_rule(rule, 0);

    let filter = ptype
        .eq(pt)
        .and(v0.eq(&rule[0]))
        .and(v1.eq(&rule[1]))
        .and(v2.eq(&rule[2]))
        .and(v3.eq(&rule[3]))
        .and(v4.eq(&rule[4]))
        .and(v5.eq(&rule[5]));

    diesel::delete(casbin_rule.filter(filter))
        .execute(conn)
        .map(|n| n == 1)
        .map_err(DatabaseError::from)
}

#[tracing::instrument(err, skip_all)]
pub fn remove_policies(conn: &mut DbConnection, pt: &str, rules: Vec<Vec<String>>) -> Result<bool> {
    conn.transaction(|conn| {
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

            match diesel::delete(casbin_rule.filter(filter)).execute(conn) {
                Ok(n) if n == 1 => continue,
                _ => return Err(DieselError::RollbackTransaction.into()),
            }
        }

        Ok(true)
    })
}

#[tracing::instrument(err, skip_all)]
pub fn remove_filtered_policy(
    conn: &mut DbConnection,
    pt: &str,
    field_index: usize,
    field_values: Vec<String>,
) -> Result<bool> {
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
        .execute(conn)
        .map(|n| n >= 1)
        .map_err(DatabaseError::from)
}

#[tracing::instrument(err, skip_all)]
pub fn clear_policy(conn: &mut DbConnection) -> Result<()> {
    diesel::delete(casbin_rule)
        .execute(conn)
        .map(|_| ())
        .map_err(DatabaseError::from)
}

#[tracing::instrument(err, skip_all)]
pub fn save_policy(conn: &mut DbConnection, rules: Vec<NewCasbinRule>) -> Result<()> {
    conn.transaction::<_, DatabaseError, _>(|conn| {
        diesel::delete(casbin_rule).execute(conn)?;

        diesel::insert_into(casbin_rule)
            .values(&rules)
            .execute(conn)?;
        Ok(())
    })
}

#[tracing::instrument(err, skip_all)]
pub fn load_policy(conn: &mut DbConnection) -> Result<Vec<CasbinRule>> {
    casbin_rule
        .load::<CasbinRule>(conn)
        .map_err(DatabaseError::from)
}

#[tracing::instrument(err, skip_all)]
pub fn add_policy(conn: &mut DbConnection, new_rule: NewCasbinRule) -> Result<()> {
    diesel::insert_into(casbin_rule)
        .values(&new_rule)
        .execute(conn)?;
    Ok(())
}

#[tracing::instrument(err, skip_all)]
pub fn add_policies(conn: &mut DbConnection, new_rules: Vec<NewCasbinRule>) -> Result<()> {
    diesel::insert_into(casbin_rule)
        .values(&new_rules)
        .execute(conn)?;
    Ok(())
}

fn normalize_casbin_rule(mut rule: Vec<String>, field_index: usize) -> Vec<String> {
    rule.resize(6 - field_index, String::new());
    rule
}
