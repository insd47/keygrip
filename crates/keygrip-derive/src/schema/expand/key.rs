use super::super::attributes::Key;
use super::super::field::Field;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

pub struct Primary {
    pub partition: String,
    pub sort: Option<String>,
    pub bindings: Vec<Ident>,
    pub parts: TokenStream,
    pub value: TokenStream,
    pub space: TokenStream,
}

pub fn primary(key: &Key, keygrip: &TokenStream) -> Primary {
    let names = key.names();
    let fields = key.partition.iter().chain(&key.sort).collect::<Vec<_>>();
    let bindings = (0..fields.len())
        .map(|index| format_ident!("__key_{index}"))
        .collect::<Vec<_>>();
    let partition_bindings = &bindings[..key.partition.len()];
    let sort_bindings = &bindings[key.partition.len()..];
    let partition_value = joined(partition_bindings);
    let parts = if let Some(sort) = &names.sort {
        let literals = key
            .literals
            .iter()
            .map(|literal| quote!(::std::string::String::from(#literal)));
        let bindings = sort_bindings.iter().map(|binding| quote!(#binding));
        let sort_value = joined_values(&literals.chain(bindings).collect::<Vec<_>>());
        let partition = &names.partition;

        quote!(#keygrip::Parts::two(#partition, #partition_value, #sort, #sort_value))
    } else {
        let partition = &names.partition;

        quote!(#keygrip::Parts::one(#partition, #partition_value))
    };
    let values = fields.into_iter().map(|field| quote!(&self.#field)).collect::<Vec<_>>();
    let value = tuple(&values);
    let space = space(key, keygrip);

    Primary {
        partition: names.partition,
        sort: names.sort,
        bindings,
        parts,
        value,
        space,
    }
}

/// The `SPACE` constant for a sort key with leading literals.
fn space(key: &Key, keygrip: &TokenStream) -> TokenStream {
    if key.literals.is_empty() {
        return quote!();
    }

    let literal = key
        .literals
        .iter()
        .map(syn::LitStr::value)
        .collect::<Vec<_>>()
        .join("#");
    let space = if key.sort.is_empty() {
        quote!(#keygrip::SortSpace::Exact(#literal))
    } else {
        quote!(#keygrip::SortSpace::Prefix(#literal))
    };

    quote! {
        const SPACE: ::core::option::Option<#keygrip::SortSpace> =
            ::core::option::Option::Some(#space);
    }
}

pub fn prefix(sort: &[Field], keygrip: &TokenStream) -> TokenStream {
    if sort.len() < 2 {
        return quote!();
    }

    let fields = &sort[..sort.len() - 1];
    let arguments = fields.iter().map(Field::name).collect::<Vec<_>>();
    let generics = (0..fields.len())
        .map(|index| format_ident!("P{index}"))
        .collect::<Vec<_>>();

    quote! {
        pub fn prefix<#(#generics: #keygrip::KeyPart + ?Sized),*>(#(#arguments: &#generics),*) -> String {
            [#(#keygrip::KeyPart::part(#arguments)),*, String::new()].join("#")
        }
    }
}

fn joined(fields: &[Ident]) -> TokenStream {
    if fields.len() == 1 {
        let field = &fields[0];
        quote!(#field)
    } else {
        quote!([#(#fields),*].join("#"))
    }
}

fn joined_values(values: &[TokenStream]) -> TokenStream {
    if values.len() == 1 {
        values[0].clone()
    } else {
        quote!([#(#values),*].join("#"))
    }
}

fn tuple(values: &[TokenStream]) -> TokenStream {
    if values.len() == 1 {
        values[0].clone()
    } else {
        quote!((#(#values),*))
    }
}
