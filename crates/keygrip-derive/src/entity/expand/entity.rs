use crate::entity::attributes::Entity;
use crate::entity::expand::{index, key};
use crate::entity::utils;
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub fn entity(name: &Ident, attr: &Entity) -> TokenStream {
    let keygrip = quote!(::keygrip);
    let entity_name = attr.name(name);
    let primary = key::primary(&attr.key, &keygrip);
    let partition = &primary.partition;
    let sort = utils::option(primary.sort.as_deref());
    let key_count = primary.bindings.len();
    let bindings = &primary.bindings;
    let parts = &primary.parts;
    let value = &primary.value;

    let indexes = attr
        .indexes
        .iter()
        .map(|index| index::constant(index, &keygrip))
        .collect::<Vec<_>>();

    let prefix = key::prefix(&attr.key.sort, &keygrip);

    quote! {
        impl #keygrip::Entity for #name {
            const NAME: &'static str = #entity_name;
            const PARTITION: &'static str = #partition;
            const SORT: ::core::option::Option<&'static str> = #sort;
            type Key<'a> = #keygrip::Key<#key_count>;

            fn parts<'a>(key: impl ::core::convert::Into<Self::Key<'a>>) -> #keygrip::Parts
            where
                Self: 'a,
            {
                let [#(#bindings),*] = key.into().into_values();
                #parts
            }

            fn primary(&self) -> Self::Key<'_> {
                #keygrip::Key::from(#value)
            }
        }

        impl #name {
            #(#indexes)*
            #prefix
        }
    }
}
