/// Implement [`redis::ToRedisArgs`](https://docs.rs/redis/*/redis/trait.ToRedisArgs.html) for a type that implements display
#[macro_export]
macro_rules! impl_to_redis_args {
    ($ty:ty) => {
        impl ::redis::ToRedisArgs for $ty {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + ::redis::RedisWrite,
            {
                out.write_arg_fmt(self)
            }
        }
    };
}

/// Implement [`redis::FromRedisValue`](https://docs.rs/redis/*/redis/trait.FromRedisValue.html) for a type that implements FromStr
#[macro_export]
macro_rules! impl_from_redis_value {
    ($ty:ty) => {
        impl ::redis::FromRedisValue for $ty {
            fn from_redis_value(v: &redis::Value) -> ::redis::RedisResult<$ty> {
                match *v {
                    ::redis::Value::Data(ref bytes) => {
                        let s = ::std::str::from_utf8(bytes).map_err(|_| {
                            ::redis::RedisError::from((
                                ::redis::ErrorKind::TypeError,
                                "string is not utf8",
                            ))
                        })?;

                        ::std::str::FromStr::from_str(s).map_err(|_| {
                            ::redis::RedisError::from((
                                ::redis::ErrorKind::TypeError,
                                "failed to parse string",
                            ))
                        })
                    }
                    _ => ::redis::RedisResult::Err(::redis::RedisError::from((
                        ::redis::ErrorKind::TypeError,
                        "invalid data type",
                    ))),
                }
            }
        }
    };
}

/// Implement [`redis::FromRedisValue`](https://docs.rs/redis/*/redis/trait.FromRedisValue.html) for a type that implements deserialize
#[macro_export]
macro_rules! impl_from_redis_value_de {
    ($ty:ty) => {
        impl ::redis::FromRedisValue for $ty {
            fn from_redis_value(v: &redis::Value) -> ::redis::RedisResult<$ty> {
                match *v {
                    ::redis::Value::Data(ref bytes) => {
                        serde_json::from_slice(bytes).map_err(|_| {
                            ::redis::RedisError::from((
                                ::redis::ErrorKind::TypeError,
                                "invalid data content",
                            ))
                        })
                    }
                    _ => ::redis::RedisResult::Err(::redis::RedisError::from((
                        ::redis::ErrorKind::TypeError,
                        "invalid data type",
                    ))),
                }
            }
        }
    };
}

/// Implement [`redis::ToRedisArgs`](https://docs.rs/redis/*/redis/trait.ToRedisArgs.html) for a type that implements serialize
#[macro_export]
macro_rules! impl_to_redis_args_se {
    ($ty:ty) => {
        impl ::redis::ToRedisArgs for $ty {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + ::redis::RedisWrite,
            {
                let json_val = serde_json::to_vec(self).expect("Failed to serialize");
                out.write_arg(&json_val);
            }
        }
    };
}
