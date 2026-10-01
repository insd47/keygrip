use serde::de::DeserializeOwned;
use serde::Serialize;

/// A storage model's declared key schema.
///
/// Usually implemented with `#[derive(Schema)]` and one `#[entity(…)]`
/// attribute; manual implementations remain valid for tables that break the
/// derive's conventions.
pub trait Schema: Serialize + DeserializeOwned {
    /// Display name used in error messages (derive default: the struct name
    /// with a trailing `Table` stripped).
    const NAME: &'static str;
    /// Attribute name of the partition key.
    const PARTITION: &'static str;
    /// Attribute name of the sort key, if the table has one.
    const SORT: Option<&'static str> = None;
    /// The part of the sort-key range this schema occupies (derive: leading
    /// string literals in `sk(…)`).
    ///
    /// Queries and scans stay inside it, so several item shapes can share one
    /// table. A manual implementation that sets it must encode its sort keys
    /// to match.
    const SPACE: Option<SortSpace> = None;
    /// Key-part bundle accepted by lookups (derive: [`Key<N>`] where `N`
    /// counts every `pk`/`sk` field).
    ///
    /// [`Key<N>`]: Key
    type Key;

    /// Resolves key parts into named partition/sort values, applying any
    /// composite encoding.
    fn parts(key: impl Into<Self::Key>) -> Parts;
    /// Returns this instance's own primary key parts.
    fn primary(&self) -> Self::Key;
}

/// An ordered bundle of `N` key-part strings.
///
/// Produced from single values and tuples of [`KeyPart`] references (up to
/// four), so call sites pass `"id"` or `(&user, &kind, &id)` rather than
/// constructing keys by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key<const N: usize>([String; N]);

impl<const N: usize> Key<N> {
    /// Unwraps the parts in declaration order.
    pub fn into_values(self) -> [String; N] {
        self.0
    }
}

/// A resolved primary key: attribute names paired with encoded values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts {
    pub partition: (&'static str, String),
    pub sort: Option<(&'static str, String)>,
}

impl Parts {
    /// A partition-only key.
    pub fn one(partition: &'static str, value: impl Into<String>) -> Self {
        Self {
            partition: (partition, value.into()),
            sort: None,
        }
    }

    /// A partition + sort key.
    pub fn two(
        partition: &'static str,
        partition_value: impl Into<String>,
        sort: &'static str,
        sort_value: impl Into<String>,
    ) -> Self {
        Self {
            partition: (partition, partition_value.into()),
            sort: Some((sort, sort_value.into())),
        }
    }
}

/// The sort-key range one schema occupies within a shared table.
///
/// Declared with leading string literals in `#[entity(sk(…))]`:
/// `sk("run", problem_id, id)` stores `run#{problem_id}#{id}` and occupies
/// [`Prefix("run")`](SortSpace::Prefix); `sk("gate")` stores `gate` and
/// occupies [`Exact("gate")`](SortSpace::Exact).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortSpace {
    /// Every sort key starts with this value followed by `#`.
    Prefix(&'static str),
    /// Every sort key is exactly this value: one item per partition.
    Exact(&'static str),
}

/// A global secondary index's name and key attribute names.
///
/// Declared with `#[entity(index(…))]`, which emits one constant per index;
/// pass it to [`Query::index`](crate::Query::index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Index {
    pub name: &'static str,
    pub partition: &'static str,
    pub sort: Option<&'static str>,
}

/// A value that can serve as one part of a key.
///
/// `str`/`String` pass through unchanged. Implement this for enums and other
/// encoded types to define how they appear inside keys — the encoding stays
/// ordinary, greppable code:
///
/// ```
/// # use keygrip::KeyPart;
/// # enum Kind { Submit, Test }
/// impl KeyPart for Kind {
///     fn part(&self) -> String {
///         match self { Self::Submit => "S", Self::Test => "T" }.into()
///     }
/// }
/// ```
pub trait KeyPart {
    fn part(&self) -> String;
}

impl KeyPart for str {
    fn part(&self) -> String {
        self.into()
    }
}

impl KeyPart for String {
    fn part(&self) -> String {
        self.clone()
    }
}

impl<P: KeyPart + ?Sized> From<&P> for Key<1> {
    fn from(part: &P) -> Self {
        Self([part.part()])
    }
}

impl<A: KeyPart + ?Sized, B: KeyPart + ?Sized> From<(&A, &B)> for Key<2> {
    fn from((a, b): (&A, &B)) -> Self {
        Self([a.part(), b.part()])
    }
}

impl<A: KeyPart + ?Sized, B: KeyPart + ?Sized, C: KeyPart + ?Sized> From<(&A, &B, &C)> for Key<3> {
    fn from((a, b, c): (&A, &B, &C)) -> Self {
        Self([a.part(), b.part(), c.part()])
    }
}

impl<A: KeyPart + ?Sized, B: KeyPart + ?Sized, C: KeyPart + ?Sized, D: KeyPart + ?Sized> From<(&A, &B, &C, &D)>
    for Key<4>
{
    fn from((a, b, c, d): (&A, &B, &C, &D)) -> Self {
        Self([a.part(), b.part(), c.part(), d.part()])
    }
}

#[cfg(test)]
mod tests {
    use super::{Parts, Schema, SortSpace};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(id))]
    struct UserTable {
        id: String,
    }

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(name = "Assignment", pk(problem, scope), sk(kind, id))]
    struct Record {
        problem: String,
        scope: String,
        kind: String,
        id: String,
    }

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(user_id), sk("run", problem_id, id))]
    struct RunTable {
        user_id: String,
        problem_id: String,
        id: String,
    }

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(user_id), sk("gate"))]
    struct GateTable {
        user_id: String,
        rev: i64,
    }

    #[test]
    fn encodes_leading_sort_literals_as_a_prefix_space() {
        assert_eq!(RunTable::SPACE, Some(SortSpace::Prefix("run")));
        assert_eq!(
            RunTable::parts(("user", "problem", "id")),
            Parts::two("pk", "user", "sk", "run#problem#id")
        );
        assert_eq!(RunTable::prefix("problem"), "problem#");
    }

    #[test]
    fn encodes_literal_only_sort_keys_as_an_exact_space() {
        let gate = GateTable {
            user_id: "user".into(),
            rev: 0,
        };

        assert_eq!(GateTable::SPACE, Some(SortSpace::Exact("gate")));
        assert_eq!(GateTable::PARTITION, "pk");
        assert_eq!(GateTable::SORT, Some("sk"));
        assert_eq!(GateTable::parts("user"), Parts::two("pk", "user", "sk", "gate"));
        assert_eq!(GateTable::parts(gate.primary()), GateTable::parts("user"));
    }

    #[test]
    fn leaves_schemas_without_literals_outside_any_space() {
        assert_eq!(Record::SPACE, None);
        assert_eq!(UserTable::SPACE, None);
    }

    #[test]
    fn derives_schema_names_and_single_attribute_keys() {
        assert_eq!(UserTable::NAME, "User");
        assert_eq!(UserTable::parts("user"), Parts::one("id", "user"));
    }

    #[test]
    fn preserves_composite_key_encodings_and_name_overrides() {
        assert_eq!(Record::NAME, "Assignment");
        assert_eq!(
            Record::parts(("problem", "S", "submit", "id")),
            Parts::two("pk", "problem#S", "sk", "submit#id")
        );
    }
}
