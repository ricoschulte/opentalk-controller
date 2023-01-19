// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Test utility functions for use with the module tester and the controller
pub use ::serde_json;
pub use pretty_assertions::assert_eq;

#[cfg(feature = "controller")]
pub use common::{TestContext, TestUser, ROOM_ID, USERS, USER_1, USER_2};

#[cfg(feature = "controller")]
pub mod common;

#[cfg(feature = "controller")]
pub mod redis;

#[cfg(feature = "database")]
pub mod database;

/// Helper macro to compare a `[Serialize]` implementor with a JSON literal
///
/// Asserts that the left expression equals the right JSON literal when serialized.
///
/// # Examples
///
/// ```
/// use serde::Serialize;
///
/// #[derive(Debug, Serialize)]
/// struct User {
///     name: String,
///     age: u64,
/// }
///
/// #[test]
/// fn test_user() {
///     let bob = User {
///         name: "bob".into(),
///         age: 42,
///     };
///
///     assert_eq_json!(
///         bob,
///         {
///             "name": "bob",
///             "age": 42,
///         }
///     );
/// }
/// ```
#[macro_export]
macro_rules! assert_eq_json {
    ($val:expr,$($json:tt)+) => {
        let val: $crate::serde_json::Value = $crate::serde_json::to_value(&$val).expect("Expected value to be serializable");

        $crate::assert_eq!(val, $crate::serde_json::json!($($json)+));
    };
}
