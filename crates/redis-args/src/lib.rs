// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use proc_macro::TokenStream;

mod from_redis_value;
mod to_redis_args;

/// Can be derived by structs or enums in order to allow conversion to redis args.
///
/// This can be used in different variants, either using a format string or the `serde` serialization.
///
/// # Format string
///
/// The format string is limited to plain `{name}` arguments, no extra formatting allowed.
/// This restriction is currently necessary, because detecting which fields should be
/// formatted would be significantly more difficult otherwise.
///
/// This variant can only be used with structs, either named or anonymous.
///
/// ## Examples
///
/// ### Struct with named fields
///
/// ```
/// # #[macro_use] extern crate k3k_redis_args;
/// #[derive(ToRedisArgs)]
/// #[to_redis_args(fmt = "path:to:id={id}:with:value:{value}")]
/// struct IdValue {
///    id: String,
///    value: u32,
///    count: usize,
/// }
/// ```
///
/// ### Struct with unnamed fields
///
/// ````
/// # #[macro_use] extern crate k3k_redis_args;
/// # #[macro_use] extern crate serde;
/// #[derive(ToRedisArgs)]
/// #[to_redis_args(fmt = "path:to:{}:with:{}")]
/// struct Example(u32, String);
/// ````
///
/// # Serde
///
/// Serializes to JSON using the serde serialization of any item. The item
/// must derive `serde::Serialize`.
///
/// ## Examples
///
/// ```
/// # #[macro_use] extern crate k3k_redis_args;
/// # use serde::Serialize;
/// #[derive(ToRedisArgs, Serialize)]
/// #[to_redis_args(serde)]
/// struct IdValue {
///     id: String,
///     value: u32,
///     count: usize,
/// }
/// ```
#[proc_macro_derive(ToRedisArgs, attributes(to_redis_args))]
pub fn derive_to_redis_args(input: TokenStream) -> TokenStream {
    to_redis_args::to_redis_args(input)
}

/// Can be derived by structs or enums in order to allow conversion from redis values.
///
/// This can be used in different variants, either using `FromStr` or the `serde` deserialization.
///
/// # FromStr
///
/// The item must implement the [`std::str::FromStr`] trait.
///
/// ## Example
///
/// ```
/// # #[macro_use] extern crate k3k_redis_args;
/// #[derive(FromRedisValue)]
/// #[from_redis_value(FromStr)]
/// struct IdValue {
///    id: String,
///    value: u32,
///    count: usize,
/// }
///
/// # impl std::str::FromStr for IdValue {
/// #     type Err = String;
/// #     fn from_str(s: &str)->Result<Self, Self::Err> {
/// #         unimplemented!()
/// #     }
/// # }
/// ```
///
// # Serde
///
/// Deserializes from JSON using the serde deserialization of any item. The item
/// must derive `serde::Deserialize`.
///
/// ## Example
///
/// ```
/// # #[macro_use] extern crate k3k_redis_args;
/// # #[macro_use] extern crate serde;
/// # use serde::Deserialize;
/// #[derive(FromRedisValue, Deserialize)]
/// #[from_redis_value(serde)]
/// struct IdValue {
///     id: String,
///     value: u32,
///     count: usize,
/// }
/// ```
#[proc_macro_derive(FromRedisValue, attributes(from_redis_value))]
pub fn derive_from_redis_value(input: TokenStream) -> TokenStream {
    from_redis_value::from_redis_value(input)
}
