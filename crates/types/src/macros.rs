// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

/// Allows to create one or more typed ids
///
/// Defines the type and implements a variety of traits for it to be usable with diesel.
#[macro_export]
macro_rules! diesel_newtype {
    ($($(#[$meta:meta])* $name:ident($to_wrap:ty) => $sql_type:ty $(, $kustos_prefix:literal)?),+) => {
        $(
            pub use __newtype_impl::$name;
        )+

        mod __newtype_impl {
            use diesel::backend::{Backend, RawValue};
            use diesel::{AsExpression, FromSqlRow};
            use diesel::deserialize::{self, FromSql};
            use diesel::serialize::{self, Output, ToSql};
            use serde::{Deserialize, Serialize};
            use std::fmt;

            $(

            #[derive(
                Debug,
                Clone,
                PartialEq,
                Eq,
                PartialOrd,
                Ord,
                Hash,
                Serialize,
                Deserialize,
                AsExpression,
                FromSqlRow,
            )]
            $(#[$meta])*
            #[diesel(sql_type = $sql_type)]
            #[allow(missing_docs)]
            pub struct $name($to_wrap);

            impl $name {
                /// Wrap a value into this type.
                pub const fn from(inner: $to_wrap) -> Self {
                    Self (inner)
                }

                /// Get a reference to the inner type.
                pub fn inner(&self) -> &$to_wrap {
                    &self.0
                }

                /// Destructure this type and extract the inner value.
                pub fn into_inner(self) -> $to_wrap {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl<DB> ToSql<$sql_type, DB> for $name
            where
                DB: Backend,
                $to_wrap: ToSql<$sql_type, DB>,
            {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, DB>) -> serialize::Result {
                    <$to_wrap as ToSql<$sql_type, DB>>::to_sql(&self.0, out)
                }
            }

            impl<DB> FromSql<$sql_type, DB> for $name
            where
                DB: Backend,
                $to_wrap: FromSql<$sql_type, DB>,
            {
                fn from_sql(bytes: RawValue<DB>) -> deserialize::Result<Self> {
                    <$to_wrap as FromSql<$sql_type, DB>>::from_sql(bytes).map(Self)
                }

                fn from_nullable_sql(bytes: Option<RawValue<DB>>) -> deserialize::Result<Self> {
                    <$to_wrap as FromSql<$sql_type, DB>>::from_nullable_sql(bytes).map(Self)
                }
            }

            $(
            impl ::std::str::FromStr for $name {
                type Err = kustos::ResourceParseError;

                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    s.parse().map(Self).map_err(From::from)
                }
            }

            impl ::kustos::Resource for $name {
                const PREFIX: &'static str = $kustos_prefix;
            }

            )?

            )+
        }
    };
}
