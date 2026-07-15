use super::{Index, Key};
use crate::entity::utils::fields;
use proc_macro2::Ident;
use syn::spanned::Spanned;
use syn::{DeriveInput, Error, LitStr};

pub struct Entity {
    name: Option<LitStr>,
    pub key: Key,
    pub indexes: Vec<Index>,
}

impl Entity {
    pub fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let attrs = input
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("entity"))
            .collect::<Vec<_>>();

        if attrs.len() != 1 {
            return Err(Error::new(
                input.span(),
                "Entity requires one #[entity(...)] attribute",
            ));
        }

        let mut entity = Self {
            name: None,
            key: Key::default(),
            indexes: Vec::new(),
        };

        attrs[0].parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                entity.name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("pk") {
                entity.key.partition = fields(meta)?;
            } else if meta.path.is_ident("sk") {
                entity.key.sort = fields(meta)?;
            } else if meta.path.is_ident("index") {
                entity.indexes.push(Index::parse(meta)?);
            } else {
                return Err(meta.error("expected name, pk(...), sk(...), or index(...)"));
            }

            Ok(())
        })?;

        if entity.key.partition.is_empty() {
            return Err(Error::new(input.span(), "entity requires pk(...)"));
        }

        Ok(entity)
    }

    pub fn name(&self, ident: &Ident) -> LitStr {
        if let Some(name) = &self.name {
            return name.clone();
        }

        let default = ident.to_string();
        let default = default.strip_suffix("Table").unwrap_or(&default);

        LitStr::new(default, ident.span())
    }
}
