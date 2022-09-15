use diesel::backend;
use diesel::deserialize::FromSql;
use diesel::pg::Pg;
use diesel::serialize::{IsNull, Output, ToSql};
use diesel::sql_types::Jsonb;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::io::Write;

use self::v1::ProtocolEntry;

pub mod v1;

#[derive(Debug, Clone, Deserialize, FromSqlRow, AsExpression)]
#[diesel(sql_type = Jsonb)]
pub struct Protocol {
    pub version: u8,
    pub entries: Box<RawValue>,
}

#[derive(Debug, Clone, Serialize, AsExpression)]
#[diesel(sql_type = Jsonb)]
pub struct NewProtocol {
    version: u8,
    entries: Vec<ProtocolEntry>,
}

impl NewProtocol {
    pub fn new(entries: Vec<ProtocolEntry>) -> NewProtocol {
        Self {
            version: 1,
            entries,
        }
    }
}

impl ToSql<Jsonb, Pg> for NewProtocol
where
    serde_json::Value: ToSql<Jsonb, Pg>,
{
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> diesel::serialize::Result {
        out.write_all(&[1])?;
        serde_json::to_writer(out, self)
            .map(|_| IsNull::No)
            .map_err(Into::into)
    }
}

impl FromSql<Jsonb, Pg> for Protocol {
    fn from_sql(bytes: backend::RawValue<'_, Pg>) -> diesel::deserialize::Result<Self> {
        let bytes = bytes.as_bytes();
        if bytes[0] != 1 {
            return Err("Unsupported JSONB encoding version".into());
        }
        serde_json::from_slice(&bytes[1..]).map_err(Into::into)
    }
}
