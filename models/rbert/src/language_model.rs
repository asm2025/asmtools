pub use crate::Bert;
use crate::BertBuilder;
use crate::Pooling;
use kalosm_common::*;
pub use kalosm_language_model::{
    Embedder, EmbedderCacheExt, EmbedderExt, Embedding, EmbeddingInput, EmbeddingVariant,
    ModelBuilder, VectorSpace,
};
use serde::Deserialize;
use serde::Serialize;

#[async_trait::async_trait]
impl ModelBuilder for BertBuilder {
    type Model = Bert;

    async fn start_with_loading_handler(
        self,
        loading_handler: impl FnMut(ModelLoadingProgress) + Send + 'static,
    ) -> anyhow::Result<Self::Model> {
        self.build_with_loading_handler(loading_handler).await
    }

    fn requires_download(&self) -> bool {
        true
    }
}

impl Bert {
    /// Embed a sentence with a specific pooling strategy.
    pub fn embed_with_pooling(
        &self,
        input: &str,
        pooling: Pooling,
    ) -> anyhow::Result<Embedding<BertSpace>> {
        let mut tensors = self.embed_batch_raw(vec![input], pooling)?;

        Ok(Embedding::new(tensors.pop().unwrap()))
    }

    /// Embed a batch of sentences with a specific pooling strategy.
    pub fn embed_batch_with_pooling(
        &self,
        inputs: Vec<&str>,
        pooling: Pooling,
    ) -> anyhow::Result<Vec<Embedding<BertSpace>>> {
        let tensors = self.embed_batch_raw(inputs, pooling)?;

        let mut embeddings = Vec::with_capacity(tensors.len());
        for tensor in tensors {
            embeddings.push(Embedding::new(tensor));
        }

        Ok(embeddings)
    }
}

impl Embedder for Bert {
    type VectorSpace = BertSpace;

    fn embed_for(
        &self,
        input: EmbeddingInput,
    ) -> BoxedFuture<'_, anyhow::Result<Embedding<Self::VectorSpace>>> {
        match (&*self.embedding_search_prefix, input.variant) {
            (Some(prefix), EmbeddingVariant::Query) => {
                let mut new_input = prefix.clone();
                new_input.push_str(&input.text);
                self.embed_string(new_input)
            }
            _ => self.embed_string(input.text),
        }
    }

    fn embed_vec_for(
        &self,
        inputs: Vec<EmbeddingInput>,
    ) -> BoxedFuture<'_, anyhow::Result<Vec<Embedding<Self::VectorSpace>>>> {
        let inputs = inputs
            .into_iter()
            .map(
                |input| match (&*self.embedding_search_prefix, input.variant) {
                    (Some(prefix), EmbeddingVariant::Query) => {
                        let mut new_input = prefix.clone();
                        new_input.push_str(&input.text);
                        new_input
                    }
                    _ => input.text,
                },
            )
            .collect::<Vec<_>>();
        self.embed_vec(inputs)
    }

    fn embed_string(&self, input: String) -> BoxedFuture<'_, anyhow::Result<Embedding<BertSpace>>> {
        Box::pin(async move {
        let self_clone = self.clone();
            tokio::task::spawn_blocking(move || self_clone.embed_with_pooling(&input, Pooling::CLS))
                .await?
        })
    }

    fn embed_vec(
        &self,
        inputs: Vec<String>,
    ) -> BoxedFuture<'_, anyhow::Result<Vec<Embedding<BertSpace>>>> {
        Box::pin(async move {
        let self_clone = self.clone();
            tokio::task::spawn_blocking(move || {
            let inputs_borrowed = inputs.iter().map(|s| s.as_str()).collect::<Vec<_>>();
                self_clone.embed_batch_with_pooling(inputs_borrowed, Pooling::CLS)
            })
            .await?
        })
    }
}

/// A vector space for BERT sentence embeddings.
#[derive(Serialize, Deserialize)]
pub struct BertSpace;

impl VectorSpace for BertSpace {}
