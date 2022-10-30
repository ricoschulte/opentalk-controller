// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use k3k_redis_args::ToRedisArgs;
use redis::ToRedisArgs;
use serde::Serialize;

#[test]
fn to_redis_args_fmt() {
    #[derive(Debug, ToRedisArgs)]
    #[to_redis_args(fmt = "address:street={street}:housenumber={number}")]
    pub struct Address {
        street: String,
        number: u32,
    }

    let address = Address {
        street: "baker_street".to_string(),
        number: 42,
    };

    assert_eq!(
        address.to_redis_args(),
        "address:street=baker_street:housenumber=42".to_redis_args()
    );
}

#[test]
fn to_redis_args_serde() {
    #[derive(Serialize, ToRedisArgs)]
    #[to_redis_args(serde)]
    pub struct Address {
        street: String,
        number: u32,
    }

    let address = Address {
        street: "baker_street".to_string(),
        number: 42,
    };

    let redis_arg = address.to_redis_args();
    assert_eq!(redis_arg.len(), 1);
    assert_eq!(
        String::from_utf8(redis_arg[0].clone()).unwrap(),
        serde_json::to_string(&address).unwrap()
    );
}
