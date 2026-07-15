use serde::de::DeserializeOwned;
use serde::Serialize;

pub trait Entity: Serialize + DeserializeOwned {
    const NAME: &'static str;
    const PARTITION: &'static str;
    const SORT: Option<&'static str> = None;
    type Key<'a>
    where
        Self: 'a;

    fn parts<'a>(key: impl Into<Self::Key<'a>>) -> Parts
    where
        Self: 'a;
    fn primary(&self) -> Self::Key<'_>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key<const N: usize>([String; N]);

impl<const N: usize> Key<N> {
    pub fn into_values(self) -> [String; N] {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts {
    pub partition: (&'static str, String),
    pub sort: Option<(&'static str, String)>,
}

impl Parts {
    pub fn one(partition: &'static str, value: impl Into<String>) -> Self {
        Self {
            partition: (partition, value.into()),
            sort: None,
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Index {
    pub name: &'static str,
    pub partition: &'static str,
    pub sort: Option<&'static str>,
}

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

impl<A: KeyPart + ?Sized, B: KeyPart + ?Sized, C: KeyPart + ?Sized, D: KeyPart + ?Sized>
    From<(&A, &B, &C, &D)> for Key<4>
{
    fn from((a, b, c, d): (&A, &B, &C, &D)) -> Self {
        Self([a.part(), b.part(), c.part(), d.part()])
    }
}

#[cfg(test)]
mod tests {
    use super::{Entity, Parts};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, crate::Entity)]
    #[entity(pk(id))]
    struct UserTable {
        id: String,
    }

    #[derive(Debug, Serialize, Deserialize, crate::Entity)]
    #[entity(name = "Assignment", pk(problem, scope), sk(kind, id))]
    struct Record {
        problem: String,
        scope: String,
        kind: String,
        id: String,
    }

    #[test]
    fn derives_entity_names_and_single_attribute_keys() {
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
