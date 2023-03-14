// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::schema::{external_tariffs, tariffs, users};
use crate::users::UserId;
use crate::utils::Jsonb;
use chrono::{DateTime, Utc};
use core::fmt::Debug;
use database::{DbConnection, Result};
use diesel::prelude::*;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

diesel_newtype! {
    #[derive(Copy)]
    TariffId(uuid::Uuid) => diesel::sql_types::Uuid,
    ExternalTariffId(String) => diesel::sql_types::Text
}

impl TariffId {
    pub const fn nil() -> Self {
        Self::from(uuid::Uuid::nil())
    }
}

#[derive(
    Debug, Clone, Queryable, Identifiable, Serialize, Deserialize, ToRedisArgs, FromRedisValue,
)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct Tariff {
    pub id: TariffId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub quotas: Jsonb<HashMap<String, u32>>,
    pub disabled_modules: Vec<String>,
}

impl Tariff {
    pub fn get(conn: &mut DbConnection, id: TariffId) -> Result<Self> {
        let tariff = tariffs::table.filter(tariffs::id.eq(id)).get_result(conn)?;

        Ok(tariff)
    }

    pub fn get_all(conn: &mut DbConnection) -> Result<Vec<Self>> {
        let tariffs = tariffs::table.load(conn)?;

        Ok(tariffs)
    }

    pub fn get_by_name(conn: &mut DbConnection, name: &str) -> Result<Self> {
        let query = tariffs::table.filter(tariffs::name.eq(name));

        let tariff = query.get_result(conn)?;

        Ok(tariff)
    }

    pub fn delete_by_id(conn: &mut DbConnection, id: TariffId) -> Result<()> {
        let query = diesel::delete(tariffs::table).filter(tariffs::id.eq(id));
        query.execute(conn)?;
        Ok(())
    }

    pub fn get_by_external_id(conn: &mut DbConnection, id: &ExternalTariffId) -> Result<Self> {
        let query = external_tariffs::table
            .filter(external_tariffs::external_id.eq(id))
            .inner_join(tariffs::table)
            .select(tariffs::all_columns);

        let tariff = query.get_result(conn)?;

        Ok(tariff)
    }

    pub fn get_by_user_id(conn: &mut DbConnection, id: &UserId) -> Result<Self> {
        let query = users::table
            .filter(users::id.eq(id))
            .inner_join(tariffs::table)
            .select(tariffs::all_columns);

        let tariff = query.get_result(conn)?;

        Ok(tariff)
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = tariffs)]
pub struct NewTariff {
    pub name: String,
    pub quotas: Jsonb<HashMap<String, u32>>,
    pub disabled_modules: Vec<String>,
}

impl NewTariff {
    pub fn insert(self, conn: &mut DbConnection) -> Result<Tariff> {
        let query = self.insert_into(tariffs::table);
        let tariff = query.get_result(conn)?;

        Ok(tariff)
    }
}

#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = tariffs)]
pub struct UpdateTariff {
    pub name: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub quotas: Option<Jsonb<HashMap<String, u32>>>,
    pub disabled_modules: Option<Vec<String>>,
}

impl UpdateTariff {
    pub fn apply(self, conn: &mut DbConnection, tariff_id: TariffId) -> Result<Tariff> {
        let query = diesel::update(tariffs::table.filter(tariffs::id.eq(tariff_id))).set(self);
        let tariff = query.get_result(conn)?;
        Ok(tariff)
    }
}

#[derive(Debug, Clone, Insertable, Identifiable, Queryable)]
#[diesel(primary_key(external_id))]
pub struct ExternalTariff {
    pub external_id: ExternalTariffId,
    pub tariff_id: TariffId,
}

impl ExternalTariff {
    pub fn get_all_for_tariff(
        conn: &mut DbConnection,
        tariff_id: TariffId,
    ) -> Result<Vec<ExternalTariffId>> {
        let query = external_tariffs::table
            .filter(external_tariffs::tariff_id.eq(tariff_id))
            .select(external_tariffs::external_id);
        let external_ids = query.load(conn)?;

        Ok(external_ids)
    }

    pub fn delete_all_for_tariff(conn: &mut DbConnection, tariff_id: TariffId) -> Result<()> {
        let query = diesel::delete(external_tariffs::table)
            .filter(external_tariffs::tariff_id.eq(tariff_id));
        query.execute(conn)?;
        Ok(())
    }

    pub fn delete_all_for_tariff_by_external_id(
        conn: &mut DbConnection,
        tariff_id: TariffId,
        external_ids: &[ExternalTariffId],
    ) -> Result<()> {
        let query = diesel::delete(external_tariffs::table).filter(
            external_tariffs::tariff_id
                .eq(tariff_id)
                .and(external_tariffs::external_id.eq_any(external_ids)),
        );
        query.execute(conn)?;
        Ok(())
    }

    pub fn insert(self, conn: &mut DbConnection) -> Result<()> {
        let query = self.insert_into(external_tariffs::table);
        query.execute(conn)?;

        Ok(())
    }
}
