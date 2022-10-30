// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use proc_macro::TokenStream;
use quote::quote;

pub(crate) fn from_redis_value(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as syn::DeriveInput);

    let conversion = get_from_redis_value_conversion(&ast.attrs);

    match conversion {
        FromRedisValueConversion::Serde => impl_from_redis_value_serde(&ast),
        FromRedisValueConversion::FromStr => impl_from_redis_value_from_str(&ast),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum FromRedisValueConversion {
    Serde,
    FromStr,
}

fn get_from_redis_value_conversion(attrs: &[syn::Attribute]) -> FromRedisValueConversion {
    let mut found_attr = None;
    for attr in attrs {
        if let Some(segment) = attr.path.segments.iter().next() {
            if segment.ident == "from_redis_value" {
                if found_attr.is_some() {
                    panic!("Multiple #[from_redis_value(...)] found");
                } else {
                    found_attr = Some(attr);
                }
            }
        }
    }

    if let Some(attr) = found_attr {
        return parse_from_redis_value_attribute_parameters(attr.tokens.clone());
    }

    panic!("Attribute #[from_redis_value(...)] missing for #[derive(FromRedisValue)]");
}

fn parse_from_redis_value_attribute_parameters(
    parameters: proc_macro2::TokenStream,
) -> FromRedisValueConversion {
    fn fail_with_generic_message() -> FromRedisValueConversion {
        panic!("Attribute #[from_redis_value(...)] requires either `FromStr` or `serde` parameter");
    }
    match parameters.into_iter().next() {
        Some(proc_macro2::TokenTree::Group(group)) => {
            if group.delimiter() != proc_macro2::Delimiter::Parenthesis {
                panic!("Attribute #[from_redis_value(...)] must have braces: '('");
            }
            let mut tokens = group.stream().into_iter();

            let conversion = match tokens.next() {
                Some(proc_macro2::TokenTree::Ident(ident)) if ident == "FromStr" => {
                    FromRedisValueConversion::FromStr
                }
                Some(proc_macro2::TokenTree::Ident(ident)) if ident == "serde" => {
                    FromRedisValueConversion::Serde
                }
                _ => fail_with_generic_message(),
            };

            if tokens.next().is_some() {
                panic!("Attribute #[from_redis_args(...)] does not allow additional parameters");
            }

            conversion
        }
        _ => fail_with_generic_message(),
    }
}

fn impl_from_redis_value_serde(input: &syn::DeriveInput) -> TokenStream {
    let generics = &input.generics;
    let ident = &input.ident;

    let expanded = quote! {
        impl #generics redis::FromRedisValue for #ident #generics {
            fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
                match *v {
                    redis::Value::Data(ref bytes) => serde_json::from_slice(bytes).map_err(|_| {
                        redis::RedisError::from(
                            (redis::ErrorKind::TypeError, "invalid data content")
                        )
                    }),
                    _ => redis::RedisResult::Err(
                        redis::RedisError::from(
                            (redis::ErrorKind::TypeError, "invalid data type")
                        )
                    ),
                }
            }
        }
    };
    TokenStream::from(expanded)
}

fn impl_from_redis_value_from_str(input: &syn::DeriveInput) -> TokenStream {
    let generics = &input.generics;
    let ident = &input.ident;

    let expanded = quote! {
        impl #generics redis::FromRedisValue for #ident #generics {
            fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
                match *v {
                    redis::Value::Data(ref bytes) => {
                        let s = std::str::from_utf8(bytes).map_err(|_| {
                            redis::RedisError::from(
                                (redis::ErrorKind::TypeError, "string is not utf8")
                            )
                        })?;

                        std::str::FromStr::from_str(s).map_err(|_| {
                            redis::RedisError::from(
                                (redis::ErrorKind::TypeError, "invalid data type")
                            )
                        })
                    },
                    _ => redis::RedisResult::Err(
                        redis::RedisError::from(
                            (redis::ErrorKind::TypeError, "invalid data type")
                        )
                    ),
                }
            }
        }
    };
    TokenStream::from(expanded)
}
