use std::ops::{Deref, DerefMut};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::buffer::SamplesBuffer;

use tokio::time::Instant;

use crate::AudioStream;

/// A microphone input.
pub struct MicInput {
    #[allow(dead_code)]
    host: cpal::Host,
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
}

impl Default for MicInput {
    fn default() -> Self {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .expect("Failed to get default input device");
        let config = device
            .default_input_config()
            .expect("Failed to get default input config");
        Self {
            host,
            device,
            config,
        }
    }
}

impl MicInput {
    /// The sample size in bytes.
    pub async fn record_until(
        &self,
        deadline: Instant,
    ) -> Result<SamplesBuffer<f32>, anyhow::Error> {
        let stream = self.stream()?;
        tokio::time::sleep_until(deadline).await;
        stream.reader()
    }

    /// Records audio for a given duration.
    pub fn record_until_blocking(
        &self,
        deadline: std::time::Instant,
    ) -> Result<SamplesBuffer<f32>, anyhow::Error> {
        let stream = self.stream()?;
        std::thread::sleep(deadline - std::time::Instant::now());
        stream.reader()
    }

    /// Creates a new stream of audio data from the microphone.
    pub fn stream(&self) -> Result<MicStream, anyhow::Error> {
        let err_fn = move |err| {
            eprintln!("an error occurred on stream: {}", err);
        };
        let writer = AudioStream::new(60., &self.config);
        let writer_2 = writer.clone();

        let stream = match self.config.sample_format() {
            cpal::SampleFormat::I8 => self.device.build_input_stream(
                &self.config.config(),
                move |data: &[i8], _: &_| writer_2.write(data),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => self.device.build_input_stream(
                &self.config.config(),
                move |data: &[i16], _: &_| writer_2.write(data),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I32 => self.device.build_input_stream(
                &self.config.config(),
                move |data: &[i32], _: &_| writer_2.write(data),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::F32 => self.device.build_input_stream(
                &self.config.config(),
                move |data: &[f32], _: &_| writer_2.write(data),
                err_fn,
                None,
            )?,
            sample_format => {
                return Err(anyhow::Error::msg(format!(
                    "Unsupported sample format '{sample_format}'"
                )))
            }
        };

        stream.play()?;

        Ok(MicStream {
            _audio: stream,
            writer,
        })
    }
}

/// A stream of audio data from the microphone.
pub struct MicStream {
    _audio: cpal::Stream,
    writer: AudioStream<u16>,
}

impl Deref for MicStream {
    type Target = AudioStream<u16>;

    fn deref(&self) -> &Self::Target {
        &self.writer
    }
}

impl DerefMut for MicStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.writer
    }
}
