use std::any::Any;
use std::any::TypeId;

use super::{EmbeddingIndexedTable, EmbeddingIndexedTableSearchResult};
use kalosm_language::prelude::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use surrealdb::sql::Id;
use surrealdb::Connection;
use surrealdb::Surreal;

/// A struct that has a document associated with it.
pub trait HasDocument {
    /// Get the document associated with this struct.
    fn document(&self) -> &Document;
}

impl HasDocument for Document {
    fn document(&self) -> &Document {
        self
    }
}

/// A table in a surreal database that is indexed by embeddings from a vector database.
pub struct DocumentTable<C: Connection, R, M: Embedder, K: Chunker> {
    embedding_model: M,
    chunker: K,
    table: EmbeddingIndexedTable<C, R, M::VectorSpace>,
}

impl<C: Connection, R, M: Embedder, K: Chunker> DocumentTable<C, R, M, K> {
    /// Create a new document table.
    pub fn new(
        embedding_model: M,
        table: EmbeddingIndexedTable<C, R, M::VectorSpace>,
        chunker: K,
    ) -> Self {
        Self {
            embedding_model,
            table,
            chunker,
        }
    }

    /// Get the raw table.
    pub fn table(&self) -> &EmbeddingIndexedTable<C, R, M::VectorSpace> {
        &self.table
    }

    /// Get the raw embedding model.
    pub fn embedding_model(&self) -> &M {
        &self.embedding_model
    }

    /// Get the raw embedding model mutably.
    pub fn embedding_model_mut(&mut self) -> &mut M {
        &mut self.embedding_model
    }

    /// Insert a new record into the table with the given embedding.
    pub async fn insert(&self, value: R) -> anyhow::Result<Id>
    where
        R: HasDocument + Serialize + DeserializeOwned,
    {
        let embeddings = self
            .chunker
            .chunk(value.document(), &self.embedding_model)
            .await?;
        self.table
            .insert(
                embeddings
                    .into_iter()
                    .flat_map(|embedding| embedding.embeddings),
                value,
            )
            .await
    }

    /// Update a record in the table with the given embedding id.
    pub async fn update(&self, id: Id, value: R) -> anyhow::Result<Option<R>>
    where
        R: Serialize + DeserializeOwned,
    {
        self.table.update(id, value).await
    }

    /// Select a record from the table with the given embedding id.
    pub async fn select(&self, id: Id) -> anyhow::Result<R>
    where
        R: Serialize + DeserializeOwned,
    {
        self.table.select(id).await
    }

    /// Delete a record from the table with the given embedding id.
    pub async fn delete(&self, id: Id) -> anyhow::Result<Option<R>>
    where
        R: Serialize + DeserializeOwned,
    {
        self.table.delete(id).await
    }

    /// Select all records from the table.
    pub async fn select_all(&self) -> anyhow::Result<Vec<R>>
    where
        R: Serialize + DeserializeOwned,
    {
        self.table.select_all().await
    }

    /// Select the top k records nearest records to the given record.
    pub async fn select_nearest(
        &self,
        record: impl IntoDocument,
        k: usize,
    ) -> anyhow::Result<Vec<EmbeddingIndexedTableSearchResult<R>>>
    where
        R: DeserializeOwned,
    {
        let embedding = self
            .embedding_model
            .embed(record.into_document().await?.body())
            .await?;
        self.select_nearest_embedding(embedding, k).await
    }

    /// Select the top k records nearest records to the given embedding.
    pub async fn select_nearest_embedding(
        &self,
        embedding: Embedding<M::VectorSpace>,
        k: usize,
    ) -> anyhow::Result<Vec<EmbeddingIndexedTableSearchResult<R>>>
    where
        R: DeserializeOwned,
    {
        self.table.select_nearest(embedding, k).await
    }
}

/// A builder for creating a new document table.
pub struct DocumentTableBuilder<C: Connection, E, K: Chunker> {
    table: String,
    db: Surreal<C>,
    embedding_model: Option<E>,
    chunker: K,
    location: Option<std::path::PathBuf>,
}

impl<C: Connection> DocumentTableBuilder<C, Bert, ChunkStrategy> {
    /// Create a new document table builder.
    pub(crate) fn new(table: &str, db: Surreal<C>) -> Self {
        Self {
            table: table.to_string(),
            db,
            location: None,
            chunker: ChunkStrategy::Sentence {
                sentence_count: 1,
                overlap: 0,
            },
            embedding_model: None,
        }
    }
}

impl<C: Connection, E, K: Chunker> DocumentTableBuilder<C, E, K> {
    /// Set the location of the vector database.
    pub fn at(mut self, location: impl AsRef<std::path::Path>) -> Self {
        self.location = Some(location.as_ref().to_path_buf());
        self
    }

    /// Set the embedding model for the table.
    pub fn with_embedding_model(mut self, embedding_model: E) -> Self {
        self.embedding_model = Some(embedding_model);
        self
    }

    /// Set the chunking strategy for the table.
    pub fn with_chunker(mut self, chunker: K) -> Self {
        self.chunker = chunker;
        self
    }

    /// Build the document table.
    pub async fn build<R: Serialize + DeserializeOwned>(
        self,
    ) -> anyhow::Result<DocumentTable<C, R, E, K>>
    where
        E: Embedder,
    {
        let vector_db = if let Some(location) = self.location {
            VectorDB::new_at(location)?
        } else {
            VectorDB::new()?
        };
        let table = EmbeddingIndexedTable {
            table: self.table.to_string(),
            db: self.db,
            vector_db,
            phantom: std::marker::PhantomData,
        };
        let embedding_model = match self.embedding_model {
            Some(embedding_model) => embedding_model,
            None => {
                if TypeId::of::<E>() == TypeId::of::<Bert>() {
                    let embedding_model = Bert::builder().build().await?;
                    *(Box::new(embedding_model) as Box<dyn Any>)
                        .downcast::<E>()
                        .unwrap()
                } else {
                    return Err(anyhow::anyhow!("No embedding model provided"));
                }
            }
        };
        Ok(DocumentTable::new(embedding_model, table, self.chunker))
    }
}
