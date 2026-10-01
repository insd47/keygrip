use crate::write::{delete, put, update};
use crate::{Delete, Error, Put, Result, Schema, Update};
use aws_sdk_dynamodb::types::{self, TransactWriteItem};
use aws_sdk_dynamodb::Client;

/// One write compiled into a transaction item, with the client of the
/// [`Entity`](crate::Entity) it came from.
///
/// Converted from a [`Put`], [`Update`], or [`Delete`]; a write that fails to
/// compile carries its error to [`Transaction::run`](super::Transaction::run).
#[derive(Debug)]
pub struct Step {
    pub(super) item: Result<TransactWriteItem>,
    pub(super) client: Client,
}

impl<E: Schema> From<Put<'_, E>> for Step {
    fn from(write: Put<'_, E>) -> Self {
        let client = write.client().clone();

        Self::new(
            client,
            write.compile().and_then(|put::Compiled { table, item, clause }| {
                let put = types::Put::builder()
                    .table_name(table)
                    .set_item(Some(item))
                    .set_condition_expression(clause.condition)
                    .set_expression_attribute_names(clause.names)
                    .set_expression_attribute_values(clause.values)
                    .build()
                    .map_err(invalid)?;

                Ok(TransactWriteItem::builder().put(put).build())
            }),
        )
    }
}

impl<E: Schema> From<Update<'_, E>> for Step {
    fn from(write: Update<'_, E>) -> Self {
        let client = write.client().clone();

        Self::new(
            client,
            write.compile().and_then(
                |update::Compiled {
                     table,
                     key,
                     update,
                     clause,
                 }| {
                    let update = types::Update::builder()
                        .table_name(table)
                        .set_key(Some(key))
                        .update_expression(update)
                        .set_condition_expression(clause.condition)
                        .set_expression_attribute_names(clause.names)
                        .set_expression_attribute_values(clause.values)
                        .build()
                        .map_err(invalid)?;

                    Ok(TransactWriteItem::builder().update(update).build())
                },
            ),
        )
    }
}

impl<E: Schema> From<Delete<'_, E>> for Step {
    fn from(write: Delete<'_, E>) -> Self {
        let client = write.client().clone();

        Self::new(
            client,
            write.compile().and_then(|delete::Compiled { table, key, clause }| {
                let delete = types::Delete::builder()
                    .table_name(table)
                    .set_key(Some(key))
                    .set_condition_expression(clause.condition)
                    .set_expression_attribute_names(clause.names)
                    .set_expression_attribute_values(clause.values)
                    .build()
                    .map_err(invalid)?;

                Ok(TransactWriteItem::builder().delete(delete).build())
            }),
        )
    }
}

impl Step {
    fn new(client: Client, item: Result<TransactWriteItem>) -> Self {
        Self { item, client }
    }
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}
