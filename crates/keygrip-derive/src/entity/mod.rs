use proc_macro2::TokenStream;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Error, Fields};

mod expand;
mod field;
mod attributes;
mod utils;

pub fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    validate(&input)?;
    let attr = attributes::Entity::parse(&input)?;

    Ok(expand::entity(&input.ident, &attr))
}

fn validate(input: &DeriveInput) -> syn::Result<()> {
    match &input.data {
        Data::Struct(data) if matches!(data.fields, Fields::Named(_)) => Ok(()),
        Data::Struct(_) => Err(Error::new(input.span(), "Entity requires named fields")),
        _ => Err(Error::new(
            input.span(),
            "Entity can only be derived for structs",
        )),
    }
}
