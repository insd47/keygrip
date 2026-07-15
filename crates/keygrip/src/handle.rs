use super::query;
use crate::key::document_key;
use crate::{item, request, Entity, Error, KeyPart, Query, Result};
use aws_sdk_dynamodb::types::KeysAndAttributes;
use aws_sdk_dynamodb::Client;
use std::collections::HashMap;
use std::marker::PhantomData;

#[derive(Debug, Clone)]
/// 한 DynamoDB 엔티티의 공통 키 기반 연산을 제공한다.
pub struct Handle<E: Entity> {
    client: Client,
    name: String,
    marker: PhantomData<E>,
}

impl<E: Entity> Handle<E> {
    pub fn new(client: Client, name: impl Into<String>) -> Self {
        Self {
            client,
            name: name.into(),
            marker: PhantomData,
        }
    }

    /// 이 handle이 사용하는 DynamoDB client를 반환한다.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// 이 handle이 사용하는 DynamoDB table 이름을 반환한다.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 키에 해당하는 엔티티를 일관 읽기로 조회한다.
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

    /// 키에 해당하는 엔티티를 조회하고 없으면 NotFound를 반환한다.
    pub async fn get<'a>(&self, key: impl Into<E::Key<'a>>) -> Result<E>
    where
        E: 'a,
    {
        self.find(key)
            .await?
            .ok_or_else(|| Error::NotFound(format!("{} not found.", E::NAME)))
    }

    /// 같은 기본 키가 없을 때만 엔티티를 생성한다.
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

    /// 엔티티를 현재 값으로 저장한다.
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

    /// 키에 해당하는 엔티티를 삭제한다.
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

    /// 테이블 전체를 페이지 끝까지 읽는다.
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

    /// 최대 100개씩 나눠 여러 기본 키를 일관 읽기 한다.
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

    /// 파티션 키에 제한된 쿼리를 시작한다.
    pub fn query<P: KeyPart + ?Sized>(&self, partition: &P) -> Query<'_, E> {
        query::new(self, partition.part())
    }
}
