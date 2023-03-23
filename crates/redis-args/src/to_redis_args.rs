// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use proc_macro::TokenStream;
use quote::quote;

pub(crate) fn to_redis_args(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as syn::DeriveInput);

    let conversion = get_to_redis_args_conversion(&ast.attrs);

    match conversion {
        ToRedisArgsConversion::Serde => impl_to_redis_args_serde(&ast),
        ToRedisArgsConversion::DirectFormat => impl_to_redis_args_fmt(&ast, "{}"),
        ToRedisArgsConversion::Format(fmt) => impl_to_redis_args_fmt(&ast, fmt.as_str()),
        ToRedisArgsConversion::Display => impl_to_redis_args_display(&ast),
    }
}

fn get_to_redis_args_conversion(attrs: &[syn::Attribute]) -> ToRedisArgsConversion {
    let mut found_attr = None;
    for attr in attrs {
        if let Some(segment) = attr.path.segments.iter().next() {
            if segment.ident == "to_redis_args" {
                if found_attr.is_some() {
                    panic!("Multiple #[to_redis_args(...)] found");
                } else {
                    found_attr = Some(attr);
                }
            }
        }
    }

    if let Some(attr) = found_attr {
        return parse_to_redis_args_attribute_parameters(attr.tokens.clone());
    }

    panic!("Attribute #[to_redis_args(...)] missing for #[derive(ToRedisArgs)]");
}

#[derive(Debug, PartialEq, Eq)]
enum ToRedisArgsConversion {
    Serde,
    DirectFormat,
    Format(String),
    Display,
}

enum Fields {
    Named(Vec<syn::Ident>),
    Unnamed(usize),
    Empty,
}

fn parse_to_redis_args_attribute_parameters(
    parameters: proc_macro2::TokenStream,
) -> ToRedisArgsConversion {
    fn fail_with_generic_message() -> ToRedisArgsConversion {
        panic!("Attribute #[to_redis_args(...)] requires either `fmt`, `fmt = \"...\"`, `serde`, or `Display`  parameter");
    }
    match parameters.into_iter().next() {
        Some(proc_macro2::TokenTree::Group(group)) => {
            if group.delimiter() != proc_macro2::Delimiter::Parenthesis {
                panic!("Attribute #[to_redis_args(...)] must have braces: '('");
            }
            let mut tokens = group.stream().into_iter();

            let conversion = match tokens.next() {
                Some(proc_macro2::TokenTree::Ident(ident)) if ident == "fmt" => {
                    ToRedisArgsConversion::DirectFormat
                }
                Some(proc_macro2::TokenTree::Ident(ident)) if ident == "serde" => {
                    ToRedisArgsConversion::Serde
                }
                Some(proc_macro2::TokenTree::Ident(ident)) if ident == "Display" => {
                    ToRedisArgsConversion::Display
                }
                _ => fail_with_generic_message(),
            };

            match tokens.next() {
                Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '=' => {
                    if conversion == ToRedisArgsConversion::Serde {
                        panic!(
                        "Attribute #[to_redis_args(serde)] does not allow additional parameters"
                    );
                    }

                    let tokens = proc_macro2::TokenStream::from_iter(tokens);
                    let s = syn::parse2::<syn::LitStr>(tokens).unwrap();
                    ToRedisArgsConversion::Format(s.value())
                }
                Some(_) => {
                    panic!("Unexpected token");
                }
                None => conversion,
            }
        }
        _ => fail_with_generic_message(),
    }
}

fn get_fields(fields: &syn::Fields) -> Fields {
    match &fields {
        syn::Fields::Named(fields) => Fields::Named(
            fields
                .named
                .iter()
                .filter_map(|field| field.ident.as_ref().cloned())
                .collect(),
        ),
        syn::Fields::Unnamed(fields) => Fields::Unnamed(fields.unnamed.len()),
        syn::Fields::Unit => Fields::Empty,
    }
}

fn impl_to_redis_args_fmt(input: &syn::DeriveInput, fmt: &str) -> TokenStream {
    let generics = &input.generics;
    let ident = &input.ident;
    match &input.data {
        syn::Data::Struct(syn::DataStruct { fields, .. }) => {
            let fields = get_fields(fields);

            match fields {
                Fields::Named(fields) => {
                    let field_args = fields.iter().filter_map(|field| {
                        if fmt.contains(&format!("{{{field}}}")) {
                            Some(quote! {
                                #field=self.#field
                            })
                        } else {
                            None
                        }
                    });
                    let expanded = quote! {
                        impl #generics ::redis::ToRedisArgs for #ident #generics {
                            fn write_redis_args<W>(&self, out: &mut W)
                            where
                                W: ?Sized + ::redis::RedisWrite,
                            {
                                out.write_arg(format!(#fmt, #(#field_args),*).as_bytes())
                            }
                        }
                    };
                    TokenStream::from(expanded)
                }
                Fields::Unnamed(count) => {
                    // A very naive and probably fragile way to get the number of arguments in the format string.
                    // Should work for most cases, but could be improved someday.
                    let num_arguments =
                        fmt.replace("{{", "").replace("}}", "").matches('{').count();

                    if num_arguments > count {
                        panic!("Too many arguments in #[redis_args] format string.")
                    }

                    let field_args = (0..num_arguments).map(|i| {
                        let index = syn::Index::from(i);
                        quote! {
                            self.#index
                        }
                    });

                    let expanded = quote! {
                        impl #generics redis::ToRedisArgs for #ident #generics {
                            fn write_redis_args<W>(&self, out: &mut W)
                            where
                                W: ?Sized + redis::RedisWrite,
                            {
                                out.write_arg(format!(#fmt, #(#field_args),*).as_bytes())
                            }
                        }
                    };
                    TokenStream::from(expanded)
                }
                Fields::Empty => {
                    panic!(
                        "The #[redis_args] attribute can only be attached to structs with fields."
                    );
                }
            }
        }
        syn::Data::Enum(_) | syn::Data::Union(_) => {
            panic!("#[to_redis_args(fmt)] can only be used with structs")
        }
    }
}

fn impl_to_redis_args_serde(input: &syn::DeriveInput) -> TokenStream {
    let generics = &input.generics;
    let ident = &input.ident;

    let expanded = quote! {
        impl #generics redis::ToRedisArgs for #ident #generics {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + redis::RedisWrite
            {
                let json_val = serde_json::to_vec(self).expect("Failed to serialize");
                out.write_arg(&json_val);
            }
        }
    };
    TokenStream::from(expanded)
}

fn impl_to_redis_args_display(input: &syn::DeriveInput) -> TokenStream {
    let generics = &input.generics;
    let ident = &input.ident;

    let expanded = quote! {
        impl #generics redis::ToRedisArgs for #ident #generics {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + redis::RedisWrite
            {
                out.write_arg_fmt(&self);
            }
        }
    };
    TokenStream::from(expanded)
}
