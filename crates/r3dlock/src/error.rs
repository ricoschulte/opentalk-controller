// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use displaydoc::Display;
use redis::RedisError;
use std::result::Result as stdResult;
use thiserror::Error;

pub type Result<T> = stdResult<T, Error>;

#[derive(Display, Error, Debug, PartialEq)]
pub enum Error {
    /// Failed to unlock because redis returned no success
    FailedToUnlock,
    /// Failed to unlock because the lock already expired in redis
    AlreadyExpired,
    /// Failed to acquire the lock
    CouldNotAcquireLock,
    /// Redis Error {0}
    Redis(#[from] RedisError),
}
