pub(crate) mod casbin;
mod schema;

pub mod migrations;
pub(crate) use self::casbin::*;

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
