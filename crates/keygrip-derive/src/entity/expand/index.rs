use super::super::attributes::Index;
use crate::entity::utils;
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn constant(index: &Index, keygrip: &TokenStream) -> TokenStream {
    let constant = format_ident!("{}", index.name.value().to_shouty_snake_case());
    let name = &index.name;
    let names = index.key.names();
    let partition = names.partition;
    let sort = utils::option(names.sort.as_deref());

    quote! {
        pub const #constant: #keygrip::Index = #keygrip::Index {
            name: #name,
            partition: #partition,
            sort: #sort,
        };
    }
}
