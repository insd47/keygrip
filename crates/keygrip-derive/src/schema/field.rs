use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

pub struct Field(Vec<Ident>);

impl Field {
    pub fn name(&self) -> &Ident {
        self.0.last().expect("field path")
    }
}

impl Parse for Field {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut segments = vec![input.parse()?];

        while input.peek(Token![.]) {
            input.parse::<Token![.]>()?;
            segments.push(input.parse()?);
        }

        Ok(Self(segments))
    }
}

impl ToTokens for Field {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let mut segments = self.0.iter();

        if let Some(first) = segments.next() {
            first.to_tokens(tokens);
        }

        for segment in segments {
            Token![.](segment.span()).to_tokens(tokens);
            segment.to_tokens(tokens);
        }
    }
}
