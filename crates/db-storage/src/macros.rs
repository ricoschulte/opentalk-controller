// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

/// Creates a diesel expression that checks if the passed $v is empty
///
/// Originally from [casbin-rs/diesel-adapter][origin]
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
///
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

/// Defines a new SQL enum
///
/// The first argument is the resulting type for use in rust code.
/// The second argument is the serialized version of the enum name.
/// The third argument is the sql type.
/// The 4th argument is the string identifier of the sql type.
///
/// After that follow the variants of the enum with the syntax of:
/// `RustVariant = byte-string`
///
/// # Example
///
/// ```rust,ignore
/// sql_enum!(
///     CustomSqlEnum,          // Name of the Rust enum name
///     "custom_sql_enum",      // Name of the type name in sql
///     CustomSqlEnumType,      // Name of the diesel enum type repr
///     {
///         Variant1 = b"variant1", // the variants with their respective sql string representation
///         Variant2 = b"variant2",
///     }
/// );
/// ```
macro_rules! sql_enum {
    ($(#[$enum_meta:meta])* $enum_ident:ident,
     $sql_type_lit:literal,
     $(#[$type_meta:meta])* $type_ident:ident,
     {$($variant_ident:ident = $variant_lit:literal),* $(,)?}
    ) => {
        $(#[$type_meta])*
        #[derive(SqlType, QueryId)]
        #[diesel(postgres_type(name = $sql_type_lit))]
        pub struct $type_ident;

        $(#[$enum_meta])*
        #[derive(Debug, Copy, Clone, FromSqlRow, AsExpression)]
        #[diesel(sql_type = $type_ident)]
        pub enum $enum_ident {
            $($variant_ident),*
        }


        impl diesel::serialize::ToSql<$type_ident, Pg> for $enum_ident {
            fn to_sql<'b>(&'b self, out: &mut ::diesel::serialize::Output<'b, '_, Pg>) -> ::diesel::serialize::Result {
                match *self {
                    $(
                        Self::$variant_ident => out.write_all($variant_lit)?,
                    )*
                }

                Ok(::diesel::serialize::IsNull::No)
            }
        }

        impl FromSql<$type_ident, Pg> for $enum_ident {
            fn from_sql(bytes: ::diesel::backend::RawValue<Pg>) -> ::diesel::deserialize::Result<Self> {
                match bytes.as_bytes() {
                    $($variant_lit => Ok(Self::$variant_ident),)*
                    _ => Err("unknown enum variant".into()),
                }
            }
        }
    };
}
