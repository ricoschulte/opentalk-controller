// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::{convert::Infallible, str::FromStr};

crate::diesel_newtype! {
    #[cfg_attr(
        feature="redis",
        derive(redis_args::ToRedisArgs, redis_args::FromRedisValue),
        to_redis_args(fmt = "{0}"),
        from_redis_value(FromStr)
    )]
    GroupName(String) => diesel::sql_types::Text
}

impl FromStr for GroupName {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s.into()))
    }
}
