//! # rbert
//!
//! A Rust wrapper for [bert sentence transformers](https://arxiv.org/abs/1908.10084) implemented in [Candle](https://github.com/huggingface/candle)
//!
//! ## Usage
//!
//! ```rust, no_run
//! use kalosm_language_model::Embedder;
//! use rbert::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut bert = Bert::builder().build()?;
//!     let sentences = vec![
//!         "Cats are cool",
//!         "The geopolitical situation is dire",
//!         "Pets are great",
//!         "Napoleon was a tyrant",
//!         "Napoleon was a great general",
//!     ];
//!     let embeddings = bert.embed_batch(&sentences).await?;
//!     println!("embeddings {:?}", embeddings);
//!
//!     // Find the cosine similarity between the first two sentences
//!     let mut similarities = vec![];
//!     let n_sentences = sentences.len();
//!     for (i, e_i) in embeddings.iter().enumerate() {
//!         for j in (i + 1)..n_sentences {
//!             let e_j = embeddings.get(j).unwrap();
//!             let cosine_similarity = e_j.cosine_similarity(e_i);
//!             similarities.push((cosine_similarity, i, j))
//!         }
//!     }
//!     similarities.sort_by(|u, v| v.0.total_cmp(&u.0));
//!     for &(score, i, j) in similarities.iter() {
//!         println!("score: {score:.2} '{}' '{}'", sentences[i], sentences[j])
//!     }
//!
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]

#[cfg(feature = "mkl")]
extern crate intel_mkl_src;

#[cfg(feature = "accelerate")]
extern crate accelerate_src;

use kalosm_common::*;

use std::sync::{Arc, RwLock};

use candle_core::{IndexOp, Tensor};
use candle_nn::VarBuilder;
use tokenizers::{Encoding, PaddingParams, Tokenizer};

mod language_model;
mod raw;
mod source;

pub use crate::language_model::*;
use crate::raw::DTYPE;
pub use crate::raw::{BertModel, Config};
pub use crate::source::*;

/// A builder for a [`Bert`] model
#[derive(Default)]
pub struct BertBuilder {
    source: BertSource,
}

impl BertBuilder {
    /// Set the source of the model
    pub fn with_source(mut self, source: BertSource) -> Self {
        self.source = source;
        self
    }

    /// Build the model
    pub async fn build(self) -> anyhow::Result<Bert> {
        self.build_with_loading_handler(ModelLoadingProgress::multi_bar_loading_indicator())
            .await
    }

    /// Build the model with a loading handler
    pub async fn build_with_loading_handler(
        self,
        loading_handler: impl FnMut(ModelLoadingProgress) + Send + 'static,
    ) -> anyhow::Result<Bert> {
        Bert::from_builder(self, loading_handler).await
    }
}

/// The pooling strategy to use when embedding text.
#[derive(Debug, Clone, Copy)]
pub enum Pooling {
    /// Take the mean embedding value for all tokens (except padding)
    Mean,
    /// Take the embedding of the CLS token for each sequence
    CLS,
}

/// A bert model
#[derive(Debug, Clone)]
pub struct Bert {
    model: Arc<BertModel>,
    tokenizer: Arc<RwLock<Tokenizer>>,
}

impl Bert {
    /// Create a new [`BertBuilder`]
    pub fn builder() -> BertBuilder {
        BertBuilder::default()
    }

    /// Create a new default bert model
    pub async fn new() -> anyhow::Result<Self> {
        Self::builder().build().await
    }

    async fn from_builder(
        builder: BertBuilder,
        mut progress_handler: impl FnMut(ModelLoadingProgress) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let BertBuilder { source } = builder;
        let BertSource {
            config,
            tokenizer,
            model,
        } = source;

        let source = format!("Config ({})", config);
        let mut create_progress = ModelLoadingProgress::downloading_progress(source);
        let config_filename = config
            .download(|progress| progress_handler(create_progress(progress)))
            .await?;
        let tokenizer_source = format!("Tokenizer ({})", tokenizer);
        let mut create_progress = ModelLoadingProgress::downloading_progress(tokenizer_source);
        let tokenizer_filename = tokenizer
            .download(|progress| progress_handler(create_progress(progress)))
            .await?;
        let model_source = format!("Model ({})", model);
        let mut create_progress = ModelLoadingProgress::downloading_progress(model_source);
        let weights_filename = model
            .download(|progress| progress_handler(create_progress(progress)))
            .await?;

        let config = std::fs::read_to_string(config_filename)?;
        let config: Config = serde_json::from_str(&config)?;

        let device = accelerated_device_if_available()?;
        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[&weights_filename], DTYPE, &device)? };
        let model = BertModel::load(vb, &config)?;
        let mut tokenizer =
            Tokenizer::from_file(&tokenizer_filename).map_err(anyhow::Error::msg)?;
        tokenizer.with_padding(None);

        Ok(Bert {
            tokenizer: Arc::new(RwLock::new(tokenizer)),
            model: Arc::new(model),
        })
    }

    /// Embed a batch of sentences
    pub(crate) fn embed_batch_raw<'a>(
        &self,
        sentences: impl IntoIterator<Item = &'a str>,
        pooling: Pooling,
    ) -> anyhow::Result<Vec<Tensor>> {
        let embedding_dim = self.model.embedding_dim();
        // The batch size limit (input length * memory per token)
        let limit = embedding_dim * 512usize.pow(2) * 2;

        // The sentences we are embedding may have a very different length. First we sort them so that similar length sentences are grouped together in the same batch to reduce the overhead of padding.
        let sentences = sentences.into_iter().collect::<Vec<_>>();
        let encodings = {
            let tokenizer_read = self.tokenizer.read().unwrap();
            tokenizer_read.encode_batch(sentences, true)
        }
        .map_err(anyhow::Error::msg)?;
        let mut encodings_with_indices = encodings.into_iter().enumerate().collect::<Vec<_>>();

        encodings_with_indices.sort_unstable_by_key(|(_, encoding)| encoding.len());

        let mut combined: Vec<Option<Tensor>> = vec![None; encodings_with_indices.len()];
        let mut chunks = Vec::new();
        let mut current_chunk_len = 0;
        let mut current_chunk_max_token_len = 0;
        let mut current_chunk_indices = Vec::new();
        let mut current_chunk_text: Vec<Encoding> = Vec::new();
        for (index, encoding) in encodings_with_indices {
            let len = encoding.get_ids().len();
            current_chunk_max_token_len = current_chunk_max_token_len.max(len);
            current_chunk_len += 1;
            let score = current_chunk_len
                * (embedding_dim * 8 + embedding_dim * current_chunk_max_token_len.pow(2));
            if score > limit {
                chunks.push((
                    std::mem::take(&mut current_chunk_indices),
                    std::mem::take(&mut current_chunk_text),
                ));
                current_chunk_max_token_len = len;
                current_chunk_len = 1;
        }
            current_chunk_indices.push(index);
            current_chunk_text.push(encoding);
        }
        // Add the last chunk even if the score isn't maxed out
        chunks.push((
            std::mem::take(&mut current_chunk_indices),
            std::mem::take(&mut current_chunk_text),
        ));

        for (indices, encodings) in chunks {
            let embeddings =
                maybe_autoreleasepool(|| self.embed_batch_raw_inner(encodings, pooling))?;
            for (i, embedding) in indices.iter().zip(embeddings) {
                combined[*i] = Some(embedding);
            }
        }
        Ok(combined.into_iter().map(|x| x.unwrap()).collect())
    }

    fn embed_batch_raw_inner(
        &self,
        mut tokens: Vec<Encoding>,
        pooling: Pooling,
    ) -> anyhow::Result<Vec<Tensor>> {
        let device = &self.model.device;
                let pp = PaddingParams {
                    strategy: tokenizers::PaddingStrategy::BatchLongest,
                    ..Default::default()
                };
        tokenizers::pad_encodings(&mut tokens, &pp).map_err(anyhow::Error::msg)?;

        let n_sentences = tokens.len();
        let token_ids = tokens
            .iter()
            .map(|tokens| {
                let tokens = tokens.get_ids().to_vec();
                Ok(Tensor::new(tokens.as_slice(), device)?)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let token_ids = Tensor::stack(&token_ids, 0)?;

        let attention_masks = tokens
            .iter()
            .map(|tokens| {
                let attention_mask = tokens.get_attention_mask();
                let attention_mask = Tensor::new(attention_mask, device)?;
                Ok(attention_mask)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let attention_mask = Tensor::stack(&attention_masks, 0)?;

        // The token type ids are only used for next sentence prediction. We can just set them to zero for embedding tasks.
        let token_type_ids = token_ids.zeros_like()?;
        let embeddings =
            self.model
                .forward(&token_ids, &token_type_ids, Some(&attention_mask), false)?;

        let (_n_sentence, n_tokens, _hidden_size) = embeddings.dims3()?;

        match pooling {
            Pooling::Mean => {
                // Take the mean embedding value for all tokens (except padding)
                let embeddings = embeddings.mul(
                    &attention_mask
                        .to_dtype(DTYPE)?
                        .unsqueeze(2)?
                        .broadcast_as(embeddings.shape())?,
                )?;
        let embeddings = (embeddings.sum(1)? / (n_tokens as f64))?;
        let embeddings = normalize_l2(&embeddings)?;
                Ok(embeddings.chunk(n_sentences, 0)?)
            }
            Pooling::CLS => {
                // Index into the first token of each sentence which is the CLS token that contains the sentence embedding
                let indexed_embeddings = embeddings.i((.., 0, ..))?;
                Ok(indexed_embeddings.chunk(n_sentences, 0)?)
            }
        }
    }
}

fn normalize_l2(v: &Tensor) -> anyhow::Result<Tensor> {
    Ok(v.broadcast_div(&v.sqr()?.sum_keepdim(1)?.sqrt()?)?)
}
