// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use diesel::deserialize::FromSql;
use diesel::pg::Pg;
use diesel::serialize::{IsNull, ToSql};
use diesel::sql_types;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::io::Write;
use types::core::UserId;

/// Trait for models that have user-ids attached to them like created_by/updated_by fields
///
/// Used to make batch requests of users after fetching some resources
///
/// Should only be implemented on references of the actual models
pub trait HasUsers {
    fn populate(self, dst: &mut Vec<UserId>);
}

impl<T, I> HasUsers for I
where
    T: HasUsers,
    I: IntoIterator<Item = T>,
{
    fn populate(self, dst: &mut Vec<UserId>) {
        for t in self {
            t.populate(dst);
        }
    }
}

/// JSONB Wrapper for any type implementing the serde `Serialize` or `Deserialize` trait
#[derive(Debug, Clone, Default, Serialize, Deserialize, FromSqlRow, AsExpression)]
#[diesel(sql_type = sql_types::Jsonb)]
pub struct Jsonb<T>(pub T);

impl<T: for<'de> Deserialize<'de>> FromSql<sql_types::Jsonb, Pg> for Jsonb<T> {
    fn from_sql(value: diesel::backend::RawValue<'_, Pg>) -> diesel::deserialize::Result<Self> {
        let bytes = value.as_bytes();
        if bytes[0] != 1 {
            return Err("Unsupported JSONB encoding version".into());
        }
        serde_json::from_slice(&bytes[1..])
            .map(Self)
            .map_err(|_| "Invalid Json".into())
    }
}

impl<T: Serialize + Debug> ToSql<sql_types::Jsonb, Pg> for Jsonb<T> {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Pg>,
    ) -> diesel::serialize::Result {
        out.write_all(&[1])?;
        serde_json::to_writer(out, &self.0)
            .map(|_| IsNull::No)
            .map_err(Into::into)
    }
}
