use crate::key::document_key;
use crate::query::SortCondition;
use crate::{item, request, Delete, Error, Expression, KeyPart, Merge, Put, Query, Result, Schema, Update};
use aws_sdk_dynamodb::types::{AttributeValue, KeysAndAttributes};
use aws_sdk_dynamodb::Client;
use std::collections::HashMap;
use std::marker::PhantomData;

/// A typed grip on one DynamoDB table, providing its common key-based
/// operations.
///
/// The model declares its key [`Schema`]; the entity owns a client and table
/// name and turns that schema into requests. Reads run immediately; writes
/// ([`put`](Entity::put), [`update`](Entity::update),
/// [`merge`](Entity::merge), [`delete`](Entity::delete)) return a value that
/// runs when awaited, or joins a
/// [`Transaction`](crate::Transaction).
#[derive(Debug, Clone)]
pub struct Entity<E: Schema> {
    client: Client,
    name: String,
    marker: PhantomData<E>,
}

impl<E: Schema> Entity<E> {
    /// Creates a live entity for `name`, cloning the given client.
    pub fn new(client: &Client, name: impl Into<String>) -> Self {
        Self {
            client: client.clone(),
            name: name.into(),
            marker: PhantomData,
        }
    }

    /// Returns the DynamoDB client this entity uses.
    ///
    /// Exposed for extension code that issues operations outside the typed
    /// surface or runs a [`Transaction`](crate::Transaction).
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns the DynamoDB table name this entity targets.
    ///
    /// Exposed for extension code, alongside [`client`](Entity::client).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fetches the entity at the given key with a consistent read, or `None`
    /// if it does not exist.
    pub async fn find<'a>(&self, primary: impl Into<E::Key<'a>>) -> Result<Option<E>>
    where
        E: 'a,
    {
        let response = self
            .client
            .get_item()
            .table_name(&self.name)
            .set_key(Some(document_key(E::parts(primary))))
            .consistent_read(true)
            .send()
            .await
            .map_err(request::unavailable)?;

        item::option(response.item)
    }

    /// Fetches the entity at the given key, or fails with
    /// [`Error::NotFound`].
    pub async fn get<'a>(&self, key: impl Into<E::Key<'a>>) -> Result<E>
    where
        E: 'a,
    {
        self.find(key)
            .await?
            .ok_or_else(|| Error::NotFound(format!("{} not found.", E::NAME)))
    }

    /// Starts a [`Put`] of the whole `value`, replacing any item at its key.
    pub fn put<'a>(&'a self, value: &'a E) -> Put<'a, E> {
        Put::new(self, value)
    }

    /// Starts an [`Update`] of the item at `primary` with `expression`.
    pub fn update<'a>(&self, primary: impl Into<E::Key<'a>>, expression: Expression) -> Update<'_, E>
    where
        E: 'a,
    {
        Update::expression(self, document_key(E::parts(primary)), expression)
    }

    /// Starts a [`Merge`] that sets every serialized non-key attribute of
    /// `value`.
    ///
    /// Attributes absent from `value` are not removed, unlike
    /// [`put`](Entity::put). Use this only when the field set is preserved
    /// across writes; [`keep`](Merge::keep) preserves selected attributes
    /// already stored.
    pub fn merge<'a>(&'a self, value: &'a E) -> Merge<'a, E> {
        Merge::new(self, value)
    }

    /// Starts a [`Delete`] of the item at `primary`.
    pub fn delete<'a>(&self, primary: impl Into<E::Key<'a>>) -> Delete<'_, E>
    where
        E: 'a,
    {
        Delete::new(self, document_key(E::parts(primary)))
    }

    /// Reads the whole table, following pagination to the end.
    ///
    /// Intended for small tables; there is deliberately no paginated scan.
    /// A schema with a [`SortSpace`](crate::SortSpace) filters the scan to
    /// its own items; the read cost still covers the whole table.
    pub async fn scan(&self) -> Result<Vec<E>> {
        let mut entities = Vec::new();
        let mut cursor = None;
        let space = match SortCondition::new(E::SPACE, None)? {
            Some(condition) => {
                let sort =
                    E::SORT.ok_or_else(|| Error::Invalid("a sort key space was declared without a sort key".into()))?;

                Some((condition, sort))
            }
            None => None,
        };

        loop {
            let mut scan = self
                .client
                .scan()
                .table_name(&self.name)
                .set_exclusive_start_key(cursor);

            if let Some((condition, sort)) = &space {
                scan = scan
                    .filter_expression(&condition.expression)
                    .expression_attribute_names("#sort", *sort);

                for (placeholder, value) in &condition.values {
                    scan = scan.expression_attribute_values(*placeholder, AttributeValue::S(value.clone()));
                }
            }

            let response = scan.send().await.map_err(request::unavailable)?;

            entities.extend(item::page(response.items)?);
            cursor = response.last_evaluated_key;

            if cursor.as_ref().is_none_or(HashMap::is_empty) {
                break;
            }
        }

        Ok(entities)
    }

    /// Reads many primary keys with consistent reads, chunked by DynamoDB's
    /// batch limit of 100.
    ///
    /// Result order is not guaranteed to match the input; fails if DynamoDB
    /// leaves keys unprocessed.
    pub async fn batch<'a, I, K>(&self, keys: I) -> Result<Vec<E>>
    where
        E: 'a,
        I: IntoIterator<Item = K>,
        K: Into<E::Key<'a>>,
    {
        let keys = keys
            .into_iter()
            .map(|primary| document_key(E::parts(primary)))
            .collect::<Vec<_>>();
        let mut entities = Vec::new();

        for keys in keys.chunks(100) {
            let request_items = KeysAndAttributes::builder()
                .set_keys(Some(keys.to_vec()))
                .consistent_read(true)
                .build()
                .map_err(request::unavailable)?;
            let response = self
                .client
                .batch_get_item()
                .request_items(&self.name, request_items)
                .send()
                .await
                .map_err(request::unavailable)?;
            let incomplete = response
                .unprocessed_keys
                .as_ref()
                .is_some_and(|tables| tables.values().any(|table| !table.keys().is_empty()));

            if incomplete {
                return Err(Error::Unavailable(format!("{} batch read was not completed.", E::NAME)));
            }

            let documents = response.responses.and_then(|mut tables| tables.remove(&self.name));
            entities.extend(item::page(documents)?);
        }

        Ok(entities)
    }

    /// Starts a [`Query`] scoped to the given partition key value.
    pub fn query<P: KeyPart + ?Sized>(&self, partition: &P) -> Query<'_, E> {
        Query::new(self, partition.part())
    }
}
