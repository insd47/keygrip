use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput, Error};

mod entity;

#[proc_macro_derive(Entity, attributes(entity))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    entity::derive(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
