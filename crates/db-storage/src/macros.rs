/// Creates a diesel expression that checks if the passed $v is empty
///
/// Originally from [casbin-rs/diesel-adapter](origin)
///
/// If it is not empty the $field must match $v
/// Basically: `($v.is_empty() OR !$v.is_empty() AND $field = $v)`
/// Example:
/// ```ignore,rust
/// diesel::delete(
///   table.filter(
///     ptype
///     .eq("p")
///     .and(db_storage::eq_empty!("", v4))
///     .and(db_storage::eq_empty!("", v5)),
///   ),
/// );
/// ```
///
/// Result:
/// ```ignore,sql
/// DELETE FROM "table" WHERE "table"."ptype" = $1 AND ($2 OR $3 AND "table"."v4" = $4) AND ($5 OR $6 AND "table"."v5" = $7) -- binds: ["p", true, false, "", true, false, ""]
/// ```
/// [origin]: https://github.com/casbin-rs/diesel-adapter/blob/c23a53ae84cb92c67ae2f2c3733a3cc07b2aaf0b/src/macros.rs
#[macro_export]
macro_rules! eq_empty {
    ($v:expr,$field:expr) => {{
        || {
            use diesel::BoolExpressionMethods;

            ::diesel::dsl::sql("")
                .bind::<diesel::sql_types::Bool, _>($v.is_empty())
                .or(diesel::dsl::sql("")
                    .bind::<diesel::sql_types::Bool, _>(!$v.is_empty())
                    .and($field.eq($v)))
        }
    }
    ()};
}

/// Allows to create one or more typed ids
///
/// Defines the type and implements a variety of traits for it to be usable with diesel.
/// See <https://stackoverflow.com/a/59948116> for more information.
#[macro_export]
macro_rules! diesel_newtype {
    ($($(#[$meta:meta])* $name:ident($to_wrap:ty) => $sql_type:ty, $sql_type_lit:literal ),+) => {
        $(
            pub use __newtype_impl::$name;
        )+

        mod __newtype_impl {
            use diesel::backend::Backend;
            use diesel::deserialize;
            use diesel::serialize::{self, Output};
            use diesel::types::{FromSql, ToSql};
            use serde::{Deserialize, Serialize};
            use std::io::Write;
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
            #[sql_type = $sql_type_lit]
            pub struct $name($to_wrap);

            impl $name {
                pub const fn from(inner: $to_wrap) -> Self {
                    Self (inner)
                }

                pub fn inner(&self) -> &$to_wrap {
                    &self.0
                }

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
                fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
                    <$to_wrap as ToSql<$sql_type, DB>>::to_sql(&self.0, out)
                }
            }

            impl<DB> FromSql<$sql_type, DB> for $name
            where
                DB: Backend,
                $to_wrap: FromSql<$sql_type, DB>,
            {
                fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
                    <$to_wrap as FromSql<$sql_type, DB>>::from_sql(bytes).map(Self)
                }
            }

            )+
        }
    };
}
