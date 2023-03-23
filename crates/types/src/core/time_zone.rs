// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use derive_more::{AsRef, Display, From, FromStr, Into};

use crate::imports::*;

/// Representation of a timezone
#[derive(AsRef, Display, From, FromStr, Into, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "diesel", derive(FromSqlRow, AsExpression), diesel(sql_type = diesel::sql_types::Text))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TimeZone(chrono_tz::Tz);

#[cfg(feature = "diesel")]
mod diesel_traits {
    use super::*;

    use std::{
        io::Write,
        str::{from_utf8, FromStr},
    };

    use chrono_tz::Tz;
    use diesel::{
        backend::RawValue,
        deserialize::{self, FromSql},
        pg::Pg,
        serialize::{self, IsNull, Output, ToSql},
    };

    impl ToSql<diesel::sql_types::Text, Pg> for TimeZone {
        fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
            write!(out, "{}", self.0)?;
            Ok(IsNull::No)
        }
    }

    impl FromSql<diesel::sql_types::Text, Pg> for TimeZone {
        fn from_sql(bytes: RawValue<Pg>) -> deserialize::Result<Self> {
            let s = from_utf8(bytes.as_bytes())?;
            let tz = Tz::from_str(s)?;

            Ok(Self(tz))
        }
    }
}
