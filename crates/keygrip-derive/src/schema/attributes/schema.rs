use super::{Index, Key};
use crate::schema::utils::fields;
use proc_macro2::Ident;
use syn::spanned::Spanned;
use syn::{DeriveInput, Error, LitStr};

pub struct Schema {
    name: Option<LitStr>,
    pub key: Key,
    pub indexes: Vec<Index>,
}

impl Schema {
    pub fn parse(input: &DeriveInput) -> syn::Result<Self> {
        let attrs = input
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("entity"))
            .collect::<Vec<_>>();

        if attrs.len() != 1 {
            return Err(Error::new(
                input.span(),
                "Schema requires one #[entity(...)] attribute",
            ));
        }

        let mut schema = Self {
            name: None,
            key: Key::default(),
            indexes: Vec::new(),
        };

        attrs[0].parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                schema.name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("pk") {
                schema.key.partition = fields(meta)?;
            } else if meta.path.is_ident("sk") {
                schema.key.sort = fields(meta)?;
            } else if meta.path.is_ident("index") {
                schema.indexes.push(Index::parse(meta)?);
            } else {
                return Err(meta.error("expected name, pk(...), sk(...), or index(...)"));
            }

            Ok(())
        })?;

        if schema.key.partition.is_empty() {
            return Err(Error::new(input.span(), "schema requires pk(...)"));
        }

        Ok(schema)
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
