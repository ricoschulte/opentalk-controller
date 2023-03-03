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

    pub fn get_by_name(conn: &mut DbConnection, name: &str) -> Result<Self> {
        let query = tariffs::table.filter(tariffs::name.eq(name));

        let tariff = query.get_result(conn)?;

        Ok(tariff)
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

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = external_tariffs)]
pub struct NewExternalTariff {
    pub external_id: ExternalTariffId,
    pub tariff_id: TariffId,
}

impl NewExternalTariff {
    pub fn insert(self, conn: &mut DbConnection) -> Result<()> {
        let query = self.insert_into(external_tariffs::table);
        query.execute(conn)?;

        Ok(())
    }
}
