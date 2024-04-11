//! # Kalosm OCR
//!
//! A rust wrapper for [TR OCR](https://huggingface.co/docs/transformers/model_doc/trocr)
//!
//! ## Usage
//!
//! ```rust, no_run
//! use kalosm_ocr::*;
//!
//! let mut model = Ocr::builder().build().unwrap();
//! let image = image::open("examples/ocr.png").unwrap();
//! let text = model
//!     .recognize_text(
//!         OcrInferenceSettings::new(image)
//!             .unwrap(),
//!     )
//!     .unwrap();
//!
//! println!("{}", text);
//! ```

#![warn(missing_docs)]
#[cfg(feature = "mkl")]
extern crate intel_mkl_src;

#[cfg(feature = "accelerate")]
extern crate accelerate_src;

mod image_processor;

use anyhow::anyhow;
use candle_core::DType;
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::trocr;
use candle_transformers::models::vit;
use hf_hub::api::sync::Api;
use image::{GenericImage, GenericImageView, ImageBuffer, Rgba};
use kalosm_common::*;
use tokenizers::Tokenizer;

/// A builder for [`Ocr`].
#[derive(Default)]
pub struct OcrBuilder {
    source: OcrSource,
}

impl OcrBuilder {
    /// Sets the source of the model.
    pub fn with_source(mut self, source: OcrSource) -> Self {
        self.source = source;
        self
    }

    /// Builds the [`Ocr`] model.
    pub async fn build(self) -> anyhow::Result<Ocr> {
        Ocr::new(self, |_| {}).await
    }

    /// Builds the [`Ocr`] model.
    pub async fn build_with_loading_handler(
        self,
        handler: impl FnMut(ModelLoadingProgress) + Send + Sync + 'static,
    ) -> anyhow::Result<Ocr> {
        Ocr::new(self, handler).await
    }
}

/// The source of the model.
pub struct OcrSource {
    model: FileSource,
    config: FileSource,
}

impl OcrSource {
    /// Creates a new [`OcrSource`].
    pub fn new(model: FileSource, config: FileSource) -> Self {
        Self { model, config }
    }

    /// Create the base model source.
    pub fn base() -> Self {
        Self::new(
            FileSource::huggingface(
                "microsoft/trocr-base-handwritten".to_string(),
                "refs/pr/3".to_string(),
                "model.safetensors".to_string(),
            ),
            FileSource::huggingface(
                "microsoft/trocr-base-handwritten".to_string(),
                "refs/pr/3".to_string(),
                "config.json".to_string(),
            ),
        )
    }

    /// Create a normal sized model source.
    pub fn large() -> Self {
        Self::new(
            FileSource::huggingface(
                "microsoft/trocr-large-handwritten".to_string(),
                "refs/pr/6".to_string(),
                "model.safetensors".to_string(),
            ),
            FileSource::huggingface(
                "microsoft/trocr-large-handwritten".to_string(),
                "refs/pr/6".to_string(),
                "config.json".to_string(),
            ),
        )
    }

    /// Create a base printed model source.
    pub fn base_printed() -> Self {
        Self::new(
            FileSource::huggingface(
                "microsoft/trocr-base-printed".to_string(),
                "refs/pr/7".to_string(),
                "model.safetensors".to_string(),
            ),
            FileSource::huggingface(
                "microsoft/trocr-base-printed".to_string(),
                "refs/pr/7".to_string(),
                "config.json".to_string(),
            ),
        )
    }

    /// Create a large printed model source.
    pub fn large_printed() -> Self {
        Self::new(
            FileSource::huggingface(
                "microsoft/trocr-large-printed".to_string(),
                "main".to_string(),
                "model.safetensors".to_string(),
            ),
            FileSource::huggingface(
                "microsoft/trocr-large-printed".to_string(),
                "main".to_string(),
                "config.json".to_string(),
            ),
        )
    }

    async fn varbuilder(
        &self,
        device: &Device,
        mut handler: impl FnMut(ModelLoadingProgress) + Send + Sync,
    ) -> anyhow::Result<VarBuilder> {
        let filename = self
            .model
            .download(|progress| {
                handler(ModelLoadingProgress::downloading(
                    format!("Model ({})", self.model),
                    progress,
                ))
            })
            .await?;
        Ok(unsafe { VarBuilder::from_mmaped_safetensors(&[filename], DType::F32, device)? })
    }

    async fn config(
        &self,
        mut handler: impl FnMut(ModelLoadingProgress) + Send + Sync,
    ) -> anyhow::Result<(vit::Config, trocr::TrOCRConfig)> {
        #[derive(Debug, Clone, serde::Deserialize)]
        struct Config {
            encoder: vit::Config,
            decoder: trocr::TrOCRConfig,
        }

        let (encoder_config, decoder_config) = {
            let config_filename = self
                .config
                .download(|progress| {
                    handler(ModelLoadingProgress::downloading(
                        format!("Config ({})", self.model),
                        progress,
                    ))
                })
                .await?;
            let config: Config = serde_json::from_reader(std::fs::File::open(config_filename)?)?;
            (config.encoder, config.decoder)
        };

        Ok((encoder_config, decoder_config))
    }
}

impl Default for OcrSource {
    fn default() -> Self {
        Self::base()
    }
}

/// Settings for running inference on [`Ocr`].
pub struct OcrInferenceSettings {
    image: ImageBuffer<image::Rgba<u8>, Vec<u8>>,
}

impl OcrInferenceSettings {
    /// Creates a new [`OcrInferenceSettings`] from an image.
    pub fn new<I: GenericImageView<Pixel = Rgba<u8>>>(input: I) -> anyhow::Result<Self> {
        let mut image = ImageBuffer::new(input.width(), input.height());
        image.copy_from(&input, 0, 0)?;
        Ok(Self { image })
    }

    /// Set the image to segment.
    pub fn set_image<I: GenericImageView<Pixel = Rgba<u8>>>(
        mut self,
        image: I,
    ) -> anyhow::Result<Self> {
        self.image = ImageBuffer::new(image.width(), image.height());
        self.image.copy_from(&image, 0, 0)?;
        Ok(self)
    }
}

/// The [segment anything](https://segment-anything.com/) model.
pub struct Ocr {
    device: Device,
    decoder: trocr::TrOCRModel,
    decoder_config: trocr::TrOCRConfig,
    processor: image_processor::ViTImageProcessor,
    tokenizer_dec: Tokenizer,
}

impl Ocr {
    /// Creates a new [`OcrBuilder`].
    pub fn builder() -> OcrBuilder {
        OcrBuilder::default()
    }

    async fn new(
        settings: OcrBuilder,
        mut handler: impl FnMut(ModelLoadingProgress) + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let OcrBuilder { source } = settings;
        let tokenizer_dec = {
            let tokenizer = Api::new()?
                .model(String::from("ToluClassics/candle-trocr-tokenizer"))
                .get("tokenizer.json")?;

            Tokenizer::from_file(&tokenizer).map_err(|e| anyhow!(e))?
        };
        let device = accelerated_device_if_available()?;

        let vb = source.varbuilder(&device, &mut handler).await?;

        let (encoder_config, decoder_config) = source.config(&mut handler).await?;

        let model = trocr::TrOCRModel::new(&encoder_config, &decoder_config, vb)?;

        let config = image_processor::ProcessorConfig::default();
        let processor = image_processor::ViTImageProcessor::new(&config);

        Ok(Self {
            device,
            decoder: model,
            processor,
            decoder_config,
            tokenizer_dec,
        })
    }

    /// Recognize text from an image. Returns the recognized text.
    ///
    /// # Example
    /// ```rust, no_run
    /// use kalosm_ocr::*;
    ///
    /// let mut model = Ocr::builder().build().unwrap();
    /// let image = image::open("examples/ocr.png").unwrap();
    /// let text = model
    ///     .recognize_text(
    ///         OcrInferenceSettings::new(image)
    ///             .unwrap(),
    ///     )
    ///     .unwrap();
    ///
    /// println!("{}", text);
    /// ```
    pub fn recognize_text(&mut self, settings: OcrInferenceSettings) -> anyhow::Result<String> {
        let OcrInferenceSettings { image } = settings;

        let image = image::DynamicImage::ImageRgba8(image);

        let image = vec![image];
        let image = self.processor.preprocess(image, &self.device)?;

        let encoder_xs = self.decoder.encoder().forward(&image)?;

        let mut logits_processor =
            candle_transformers::generation::LogitsProcessor::new(1337, None, None);

        let mut token_ids: Vec<u32> = vec![self.decoder_config.decoder_start_token_id];
        for index in 0..1000 {
            let context_size = if index >= 1 { 1 } else { token_ids.len() };
            let start_pos = token_ids.len().saturating_sub(context_size);
            let input_ids = Tensor::new(&token_ids[start_pos..], &self.device)?.unsqueeze(0)?;

            let logits = self.decoder.decode(&input_ids, &encoder_xs, start_pos)?;

            let logits = logits.squeeze(0)?;
            let logits = logits.get(logits.dim(0)? - 1)?;
            let token = logits_processor.sample(&logits)?;
            token_ids.push(token);

            if token == self.decoder_config.eos_token_id {
                break;
            }
        }

        let decoded = self
            .tokenizer_dec
            .decode(&token_ids, true)
            .map_err(|e| anyhow!(e))?;

        Ok(decoded)
    }
}
