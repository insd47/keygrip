use crate::schema::field::Field;
use heck::ToLowerCamelCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::{Error, LitStr, Token};

pub fn fields(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<Field>> {
    let content;
    syn::parenthesized!(content in meta.input);
    let fields = content.parse_terminated(Field::parse, Token![,])?;

    if fields.is_empty() {
        return Err(meta.error("key fields cannot be empty"));
    }

    Ok(fields.into_iter().collect())
}

/// Parses `sk(…)`: leading string literals, then fields.
pub fn sort(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<(Vec<LitStr>, Vec<Field>)> {
    let content;
    syn::parenthesized!(content in meta.input);
    let mut literals = Vec::new();
    let mut fields = Vec::new();

    while !content.is_empty() {
        if content.peek(LitStr) {
            let literal: LitStr = content.parse()?;

            if !fields.is_empty() {
                return Err(Error::new(
                    literal.span(),
                    "string literals must lead sk(...)",
                ));
            }

            let value = literal.value();
            let valid = !value.is_empty()
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

            if !valid {
                return Err(Error::new(
                    literal.span(),
                    "sort key literals must be non-empty ASCII letters, digits, '_' or '-'",
                ));
            }

            literals.push(literal);
        } else {
            fields.push(content.parse()?);
        }

        if content.is_empty() {
            break;
        }

        content.parse::<Token![,]>()?;
    }

    if literals.is_empty() && fields.is_empty() {
        return Err(meta.error("key fields cannot be empty"));
    }

    Ok((literals, fields))
}

pub fn attribute_name(fields: &[Field]) -> String {
    fields[0].name().to_string().to_lower_camel_case()
}

pub fn option(value: Option<&str>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}
