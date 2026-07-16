use crate::types::Sort;
use crate::{attr, item, request, Cursor, Entity, Error, Index, KeyPart, Page, Result, Schema};
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
}

pub fn new<E: Schema>(entity: &Entity<E>, partition: String) -> Query<'_, E> {
    Query {
        entity,
        partition,
        index: None,
        sort: None,
        newest: false,
    }
}

impl<E: Schema> Query<'_, E> {
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

    /// Returns items in descending sort-key order (newest first when the
    /// sort key is chronological).
    pub fn newest(mut self) -> Self {
        self.newest = true;
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
            .expression_attribute_values(":partition", attr::s(&self.partition))
            .scan_index_forward(!self.newest)
            .set_exclusive_start_key(cursor)
            .set_limit(limit);

        if let Some(condition) = &self.sort {
            let sort = sort.ok_or_else(|| {
                Error::Unavailable("A sort condition was used without a sort key.".into())
            })?;

            match condition {
                Sort::Prefix(prefix) => {
                    expression.push_str(" AND begins_with(#sort, :sort)");
                    query = query.expression_attribute_values(":sort", attr::s(prefix));
                }
                Sort::Equal(value) => {
                    expression.push_str(" AND #sort = :sort");
                    query = query.expression_attribute_values(":sort", attr::s(value));
                }
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
