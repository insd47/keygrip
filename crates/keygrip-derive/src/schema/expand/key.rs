use super::super::attributes::Key;
use super::super::field::Field;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

/// The generated pieces of a schema's primary key.
pub struct Primary {
    pub partition: String,
    pub sort: Option<String>,
    pub bindings: Vec<Ident>,
    pub parts: TokenStream,
    pub value: TokenStream,
    /// The `SPACE` constant, empty without sort key literals.
    pub space: TokenStream,
    /// The `prefix` builder, empty unless the sort key has two or more fields.
    pub prefix: TokenStream,
}

impl Primary {
    pub fn new(key: &Key, keygrip: &TokenStream) -> Self {
        let names = key.names();
        let fields = key.partition.iter().chain(&key.sort).collect::<Vec<_>>();
        let bindings = (0..fields.len())
            .map(|index| format_ident!("__key_{index}"))
            .collect::<Vec<_>>();
        let tokens = bindings.iter().map(|binding| quote!(#binding)).collect::<Vec<_>>();
        let (partition_tokens, sort_tokens) = tokens.split_at(key.partition.len());
        let partition_value = joined(partition_tokens);
        let partition = &names.partition;

        let parts = if let Some(sort) = &names.sort {
            let literals = key
                .literals
                .iter()
                .map(|literal| quote!(::std::string::String::from(#literal)));
            let sort_value = joined(&literals.chain(sort_tokens.iter().cloned()).collect::<Vec<_>>());

            quote!(#keygrip::Parts::two(#partition, #partition_value, #sort, #sort_value))
        } else {
            quote!(#keygrip::Parts::one(#partition, #partition_value))
        };

        let values = fields.into_iter().map(|field| quote!(&self.#field)).collect::<Vec<_>>();

        Self {
            partition: names.partition,
            sort: names.sort,
            bindings,
            parts,
            value: tuple(&values),
            space: space(key, keygrip),
            prefix: prefix(&key.sort, keygrip),
        }
    }
}

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
        const SPACE: ::core::option::Option<#keygrip::SortSpace> = ::core::option::Option::Some(#space);
    }
}

fn prefix(sort: &[Field], keygrip: &TokenStream) -> TokenStream {
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

fn joined(values: &[TokenStream]) -> TokenStream {
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
