// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

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
        let val: ::serde_json::Value = ::serde_json::to_value(&$val).expect("Expected value to be serializable");

        assert_eq!(val, ::serde_json::json!($($json)+));
    };
}
