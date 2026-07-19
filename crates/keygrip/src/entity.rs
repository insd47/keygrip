use super::query;
use crate::key::document_key;
use crate::{item, request, Error, KeyPart, Query, Result, Schema};
use aws_sdk_dynamodb::types::KeysAndAttributes;
use aws_sdk_dynamodb::Client;
use std::collections::HashMap;
use std::marker::PhantomData;

/// A typed grip on one DynamoDB table, providing its common key-based
/// operations.
///
/// The model declares its key [`Schema`]; the entity owns a client and table
/// name and turns that schema into requests. Domain-specific invariants remain
/// in extension code, which can issue conditional [`update`](Entity::update)
/// and [`store`](Entity::store) operations, merge stable item shapes with
/// [`merge`](Entity::merge), or assemble atomic writes with
/// [`transaction`](crate::transaction).
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
    /// surface or runs a [`Transaction`](crate::transaction::Transaction).
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

    /// Writes the entity only if no item with the same primary key exists;
    /// fails with [`Error::Conflict`] otherwise.
    pub async fn create(&self, entity: &E) -> Result<()> {
        let key = document_key(E::parts(entity.primary()));
        let mut names = key.keys().collect::<Vec<_>>();
        names.sort_unstable();
        let condition = names
            .into_iter()
            .map(|name| format!("attribute_not_exists({name})"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut document = item::to(entity)?;
        document.extend(key);

        self.client
            .put_item()
            .table_name(&self.name)
            .set_item(Some(document))
            .condition_expression(condition)
            .send()
            .await
            .map_err(|error| request::conflict(error, "The item already exists."))?;

        Ok(())
    }

    /// Writes the entity unconditionally, replacing any existing item.
    pub async fn put(&self, entity: &E) -> Result<()> {
        let mut document = item::to(entity)?;
        document.extend(document_key(E::parts(entity.primary())));

        self.client
            .put_item()
            .table_name(&self.name)
            .set_item(Some(document))
            .send()
            .await
            .map_err(request::unavailable)?;

        Ok(())
    }

    /// Deletes the item at the given key; succeeds even if it did not exist.
    pub async fn delete<'a>(&self, primary: impl Into<E::Key<'a>>) -> Result<()>
    where
        E: 'a,
    {
        self.client
            .delete_item()
            .table_name(&self.name)
            .set_key(Some(document_key(E::parts(primary))))
            .send()
            .await
            .map_err(request::unavailable)?;

        Ok(())
    }

    /// Reads the whole table, following pagination to the end.
    ///
    /// Intended for small tables; there is deliberately no paginated scan.
    pub async fn scan(&self) -> Result<Vec<E>> {
        let mut entities = Vec::new();
        let mut cursor = None;

        loop {
            let response = self
                .client
                .scan()
                .table_name(&self.name)
                .set_exclusive_start_key(cursor)
                .send()
                .await
                .map_err(request::unavailable)?;

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
                return Err(Error::Unavailable(format!(
                    "{} batch read was not completed.",
                    E::NAME
                )));
            }

            let documents = response
                .responses
                .and_then(|mut tables| tables.remove(&self.name));
            entities.extend(item::page(documents)?);
        }

        Ok(entities)
    }

    /// Starts a [`Query`] scoped to the given partition key value.
    pub fn query<P: KeyPart + ?Sized>(&self, partition: &P) -> Query<'_, E> {
        query::new(self, partition.part())
    }
}
