use crate::{item, request, Cursor, Entity, Error, Index, KeyPart, Page, Result, Schema, SortSpace};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

/// A typed transliteration of the DynamoDB Query API.
///
/// Built by [`Entity::query`]; constrained to what a single Query call can
/// express natively — a partition equality, at most one sort-key condition,
/// a direction, and pagination. Attribute names come from the entity's key
/// schema, never from strings at the call site.
pub struct Query<'e, E: Schema> {
    entity: &'e Entity<E>,
    partition: String,
    index: Option<&'static Index>,
    sort: Option<Sort>,
    newest: bool,
    consistent: bool,
}

impl<'e, E: Schema> Query<'e, E> {
    pub(crate) fn new(entity: &'e Entity<E>, partition: String) -> Self {
        Self {
            entity,
            partition,
            index: None,
            sort: None,
            newest: false,
            consistent: false,
        }
    }

    /// Targets a global secondary index declared on the entity
    /// (`#[entity(index(…))]`) instead of the primary key.
    pub fn index(mut self, index: &'static Index) -> Self {
        self.index = Some(index);
        self
    }

    /// Constrains the sort key with `begins_with(prefix)`.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.sort = Some(Sort::Prefix(prefix.into()));
        self
    }

    /// Constrains the sort key to an exact value.
    pub fn eq<P: KeyPart + ?Sized>(mut self, value: &P) -> Self {
        self.sort = Some(Sort::Equal(value.part()));
        self
    }

    /// Constrains the sort key to values strictly greater than `value` —
    /// the cursor idiom for resuming a chronological partition.
    pub fn after<P: KeyPart + ?Sized>(mut self, value: &P) -> Self {
        self.sort = Some(Sort::After(value.part()));
        self
    }

    /// Returns items in descending sort-key order (newest first when the
    /// sort key is chronological).
    pub fn newest(mut self) -> Self {
        self.newest = true;
        self
    }

    /// Reads with strong consistency, so the results reflect every write
    /// that succeeded before the query.
    ///
    /// Global secondary indexes do not support consistent reads; combined
    /// with [`index`](Self::index), the query fails with
    /// [`Error::Invalid`] before it is sent.
    pub fn consistent(mut self) -> Self {
        self.consistent = true;
        self
    }

    /// Runs the query and returns one page of at most `limit` items.
    ///
    /// Pass the previous page's [`cursor`](Page::cursor) to resume.
    pub async fn page(self, cursor: Option<Cursor>, limit: i32) -> Result<Page<E>> {
        self.send(cursor, Some(limit)).await
    }

    /// Runs the query and drains every page into one vector.
    pub async fn all(self) -> Result<Vec<E>> {
        let mut entities = Vec::new();
        let mut cursor = None;

        loop {
            let page = self.send(cursor, None).await?;
            entities.extend(page.items);
            cursor = page.cursor;

            if cursor.as_ref().is_none_or(HashMap::is_empty) {
                break;
            }
        }

        Ok(entities)
    }

    async fn send(&self, cursor: Option<Cursor>, limit: Option<i32>) -> Result<Page<E>> {
        if self.consistent && self.index.is_some() {
            return Err(Error::Invalid(
                "a global secondary index does not support consistent reads".into(),
            ));
        }

        let partition = self.index.map_or(E::PARTITION, |index| index.partition);
        let sort = self.index.map_or(E::SORT, |index| index.sort);
        let mut expression = "#partition = :partition".to_string();
        let mut query = self
            .entity
            .client()
            .query()
            .table_name(self.entity.name())
            .set_index_name(self.index.map(|index| index.name.to_string()))
            .key_condition_expression(&expression)
            .expression_attribute_names("#partition", partition)
            .expression_attribute_values(":partition", AttributeValue::S(self.partition.clone()))
            .scan_index_forward(!self.newest)
            .consistent_read(self.consistent)
            .set_exclusive_start_key(cursor)
            .set_limit(limit);

        // The table's sort key space does not apply to an index's sort key.
        let space = self.index.map_or(E::SPACE, |_| None);

        if let Some(condition) = SortCondition::new(space, self.sort.as_ref())? {
            let sort = sort.ok_or_else(|| Error::Invalid("a sort condition was used without a sort key".into()))?;

            expression.push_str(" AND ");
            expression.push_str(&condition.expression);

            for (placeholder, value) in condition.values {
                query = query.expression_attribute_values(placeholder, AttributeValue::S(value));
            }

            query = query
                .key_condition_expression(&expression)
                .expression_attribute_names("#sort", sort);
        }

        let response = query.send().await.map_err(request::unavailable)?;

        Ok(Page {
            items: item::page(response.items)?,
            cursor: response.last_evaluated_key,
        })
    }
}

/// Sort-key constraint of a [`Query`].
pub enum Sort {
    Prefix(String),
    Equal(String),
    After(String),
}

/// A sort-key condition over `#sort`, combining the schema's
/// [`SortSpace`] with the query's own constraint.
#[derive(Debug, PartialEq, Eq)]
pub struct SortCondition {
    pub expression: String,
    pub values: Vec<(&'static str, String)>,
}

impl SortCondition {
    pub fn new(space: Option<SortSpace>, sort: Option<&Sort>) -> Result<Option<Self>> {
        let condition = match (space, sort) {
            (None, None) => return Ok(None),
            (None, Some(Sort::Prefix(prefix))) => Self::begins_with(prefix.clone()),
            (None, Some(Sort::Equal(value))) => Self::equal(value.clone()),
            (None, Some(Sort::After(value))) => Self {
                expression: "#sort > :sort".into(),
                values: vec![(":sort", value.clone())],
            },
            (Some(SortSpace::Prefix(space)), sort) => {
                let base = format!("{space}#");

                match sort {
                    None => Self::begins_with(base),
                    Some(Sort::Prefix(prefix)) => Self::begins_with(format!("{base}{prefix}")),
                    Some(Sort::Equal(value)) => Self::equal(format!("{base}{value}")),
                    // A key condition takes one comparison, so "after `value`, inside the
                    // space" is a range: NUL is the smallest suffix, and `$` follows the `#`
                    // that ends the space.
                    Some(Sort::After(value)) => Self {
                        expression: "#sort BETWEEN :low AND :high".into(),
                        values: vec![(":low", format!("{base}{value}\u{0}")), (":high", format!("{space}$"))],
                    },
                }
            }
            (Some(SortSpace::Exact(space)), None) => Self::equal(space.into()),
            (Some(SortSpace::Exact(_)), Some(_)) => {
                return Err(Error::Invalid(
                    "a sort condition was used on a sort key made only of literals".into(),
                ));
            }
        };

        Ok(Some(condition))
    }

    fn begins_with(value: String) -> Self {
        Self {
            expression: "begins_with(#sort, :sort)".into(),
            values: vec![(":sort", value)],
        }
    }

    fn equal(value: String) -> Self {
        Self {
            expression: "#sort = :sort".into(),
            values: vec![(":sort", value)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Sort, SortCondition};
    use crate::{Entity, Error, Index, SortSpace};
    use aws_sdk_dynamodb::config::BehaviorVersion;
    use aws_sdk_dynamodb::{Client, Config};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, crate::Schema)]
    #[entity(pk(owner), sk(id), index(name = "byId", pk(id)))]
    struct RecordTable {
        owner: String,
        id: String,
    }

    #[test]
    fn keeps_queries_inside_a_prefix_space() {
        let space = Some(SortSpace::Prefix("run"));
        let condition = |sort: Option<Sort>| SortCondition::new(space, sort.as_ref()).unwrap().unwrap();

        assert_eq!(condition(None).values, [(":sort", "run#".into())]);
        assert_eq!(
            condition(Some(Sort::Prefix("p1#".into()))).values,
            [(":sort", "run#p1#".into())]
        );
        assert_eq!(condition(Some(Sort::Equal("p1#a".into()))).expression, "#sort = :sort");

        let after = condition(Some(Sort::After("p1#a".into())));

        assert_eq!(after.expression, "#sort BETWEEN :low AND :high");
        assert_eq!(
            after.values,
            [(":low", "run#p1#a\u{0}".into()), (":high", "run$".into())]
        );
    }

    #[test]
    fn matches_exact_spaces_and_rejects_further_sort_conditions() {
        let space = Some(SortSpace::Exact("gate"));
        let exact = SortCondition::new(space, None).unwrap().unwrap();
        let error = SortCondition::new(space, Some(&Sort::Prefix("x".into()))).unwrap_err();

        assert_eq!(exact.expression, "#sort = :sort");
        assert_eq!(exact.values, [(":sort", "gate".into())]);
        assert!(matches!(error, Error::Invalid(_)));
    }

    #[test]
    fn passes_sort_conditions_through_without_a_space() {
        assert_eq!(SortCondition::new(None, None).unwrap(), None);
        assert_eq!(
            SortCondition::new(None, Some(&Sort::After("a".into())))
                .unwrap()
                .unwrap()
                .values,
            [(":sort", "a".into())]
        );
    }

    #[tokio::test]
    async fn rejects_consistent_index_queries_before_sending() {
        let config = Config::builder().behavior_version(BehaviorVersion::latest()).build();
        let records = Entity::<RecordTable>::new(&Client::from_conf(config), "Records");
        let index: &'static Index = &RecordTable::BY_ID;
        let error = records.query("id").index(index).consistent().all().await.unwrap_err();

        assert!(matches!(error, Error::Invalid(detail) if detail.contains("consistent")));
    }
}
