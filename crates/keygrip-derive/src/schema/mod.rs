use proc_macro2::TokenStream;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Error, Fields};

mod attributes;
mod expand;
mod field;
mod utils;

pub fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    validate(&input)?;
    let attr = attributes::Schema::parse(&input)?;

    Ok(expand::schema(&input.ident, &attr))
}

fn validate(input: &DeriveInput) -> syn::Result<()> {
    match &input.data {
        Data::Struct(data) if matches!(data.fields, Fields::Named(_)) => Ok(()),
        Data::Struct(_) => Err(Error::new(input.span(), "Schema requires named fields")),
        _ => Err(Error::new(
            input.span(),
            "Schema can only be derived for structs",
        )),
    }
}
