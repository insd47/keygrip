use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput, Error};

mod schema;

/// Derives `keygrip::Schema` from a struct's `#[entity(…)]` attribute.
///
/// ```ignore
/// #[derive(Serialize, Deserialize, Schema)]
/// #[entity(pk(user_id), sk(problem_id, kind, id),
///          index(name = "byId", pk(id), sk(user_id)))]
/// struct ExecutionTable { /* … */ }
/// ```
///
/// # Attribute grammar
///
/// - `pk(field, …)` — required; the field(s) composing the partition key.
/// - `sk(field, …)` — optional; the field(s) composing the sort key.
/// - `index(name = "…", pk(…), sk(…))` — repeatable; declares a global
///   secondary index and emits a `keygrip::Index` constant named after the
///   index in SHOUTY_SNAKE_CASE (`byId` → `BY_ID`).
/// - `name = "…"` — overrides the display name used in error messages
///   (default: the struct name with a trailing `Table` stripped).
///
/// # Generated code
///
/// - `impl keygrip::Schema`: attribute names, `Key<N>` (one slot per key
///   field), `parts`, and `primary`.
/// - Attribute naming: a single-field key uses the field's camelCase name
///   (`user_id` → `userId`); once any component is composite, the synthetic
///   names `pk`/`sk` are used and members are joined with `#` through
///   `keygrip::KeyPart`.
/// - For a composite sort key, a `prefix(…)` associated function taking all
///   but the last member and returning a `begins_with`-ready string
///   (`"{a}#{b}#"`).
///
/// Non-`String` key fields implement `keygrip::KeyPart` by hand to define
/// their encoding.
#[proc_macro_derive(Schema, attributes(entity))]
pub fn derive_schema(input: TokenStream) -> TokenStream {
    schema::derive(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
