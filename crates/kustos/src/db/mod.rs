// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

pub(crate) mod casbin;
mod schema;

pub mod migrations;
pub(crate) use self::casbin::*;

#[macro_export]
macro_rules! eq_empty {
    ($v:expr,$field:expr) => {{
        || {
            use ::diesel::BoolExpressionMethods;

            ::diesel::dsl::sql::<::diesel::sql_types::Bool>("")
                .bind::<::diesel::sql_types::Bool, _>($v.is_empty())
                .or(::diesel::dsl::sql::<::diesel::sql_types::Bool>("")
                    .bind::<::diesel::sql_types::Bool, _>(!$v.is_empty())
                    .and($field.eq($v)))
        }
    }
    ()};
}
