// Retrieval Strategies:
// 1. Simple keyword search
// 2. Vector search
//  1. Search by sentence, return window around match
//  2. Search by summary, return document
//  3. Search through document tree
//  4. Search by questions that may be answered by the document
//  5. Classify documents, search by class
//
// Context extraction strategies:
// 1. Dump all sentences
// 2. Dump all sentences that mention an entity
// 3. Extract relevant sentences with an llm

use kalosm_language_model::Embedder;

use crate::context::Document;

use super::Chunk;

mod chunking;
pub use chunking::*;
mod hypothetical;
pub use hypothetical::*;
mod summary;
pub use summary::*;

/// A strategy for chunking a document into smaller pieces.
#[async_trait::async_trait]
pub trait Chunker {
    /// Chunk a document into embedded snippets.
    async fn chunk<E: Embedder + Send>(
        &self,
        document: &Document,
        embedder: &E,
    ) -> anyhow::Result<Vec<Chunk<E::VectorSpace>>>;

    /// Chunk a batch of documents into embedded snippets.
    async fn chunk_batch<'a, I, E: Embedder + Send>(
        &self,
        documents: I,
        embedder: &E,
    ) -> anyhow::Result<Vec<Vec<Chunk<E::VectorSpace>>>>
    where
        I: IntoIterator<Item = &'a Document> + Send,
        I::IntoIter: Send,
    {
        let mut chunks = Vec::new();
        for document in documents {
            chunks.push(self.chunk(document, embedder).await?);
        }
        Ok(chunks)
    }
}
