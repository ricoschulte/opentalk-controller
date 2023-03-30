// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use kustos::subject::PolicyUser;

crate::diesel_newtype! {
    #[derive(Copy, redis_args::ToRedisArgs, redis_args::FromRedisValue)]
    #[to_redis_args(fmt)]
    #[from_redis_value(FromStr)]
    UserId(uuid::Uuid) => diesel::sql_types::Uuid, "/users/"
}

#[cfg(feature = "kustos")]
impl From<UserId> for PolicyUser {
    fn from(id: UserId) -> Self {
        Self::from(id.into_inner())
    }
}
