use crate::entity::field::Field;
use heck::ToLowerCamelCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::Token;

pub fn fields(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<Field>> {
    let content;
    syn::parenthesized!(content in meta.input);
    let fields = content.parse_terminated(Field::parse, Token![,])?;

    if fields.is_empty() {
        return Err(meta.error("key fields cannot be empty"));
    }

    Ok(fields.into_iter().collect())
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
