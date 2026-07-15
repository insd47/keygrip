use super::Key;
use crate::entity::utils;
use syn::LitStr;

pub struct Index {
    pub name: LitStr,
    pub key: Key,
}

impl Index {
    pub fn parse(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut key = Key::default();

        meta.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("pk") {
                key.partition = utils::fields(meta)?;
            } else if meta.path.is_ident("sk") {
                key.sort = utils::fields(meta)?;
            } else {
                return Err(meta.error("expected name, pk(...), or sk(...)"));
            }

            Ok(())
        })?;

        let name = name.ok_or_else(|| meta.error("index requires name = \"...\""))?;

        if key.partition.is_empty() {
            return Err(meta.error("index requires pk(...)"));
        }

        Ok(Self { name, key })
    }
}
