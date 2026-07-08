//! CPAL PCM 音频输出
//!
//! Provides low-level PCM audio output via the CPAL library.
//! The output stream is properly managed (not leaked) — it is stored
//! in the struct and dropped when `CpalOutput` is dropped.

use std::sync::mpsc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct CpalOutput {
    /// CPAL output stream (kept alive for the struct's lifetime).
    stream: Option<cpal::Stream>,
    /// Sender for pushing PCM samples to the audio callback.
    sender: Option<mpsc::Sender<f32>>,
    running: bool,
    /// Actual sample rate of the output stream
    sample_rate: u32,
}

impl CpalOutput {
    pub fn new() -> Self {
        CpalOutput {
            stream: None,
            sender: None,
            running: false,
            sample_rate: 44100,
        }
    }

    /// Start the CPAL output stream.
    ///
    /// Returns a clone of the sender that can be used to push samples.
    /// Returns `None` if no audio device is available or stream creation fails.
    pub fn start(&mut self) -> Option<mpsc::Sender<f32>> {
        // Don't restart if already running
        if self.running {
            return self.sender.clone();
        }

        let (tx, rx) = mpsc::channel::<f32>();
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let supported_config = device.default_output_config().ok()?;
        let channels = supported_config.channels();
        let sample_format = supported_config.sample_format();
        let config: cpal::StreamConfig = supported_config.into();
        self.sample_rate = config.sample_rate.0;

        let stream_result = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream::<f32, _, _>(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for sample in data.iter_mut().step_by(channels as usize) {
                        *sample = rx.try_recv().unwrap_or(0.0);
                    }
                },
                move |err| tracing::error!("Audio error: {}", err),
                None,
            ),
            _ => {
                tracing::warn!("Unsupported audio format, disabling audio");
                return None;
            }
        };

        match stream_result {
            Ok(stream) => {
                if let Err(e) = stream.play() {
                    tracing::warn!("Failed to play CPAL stream: {}", e);
                    return None;
                }
                self.stream = Some(stream);
                self.sender = Some(tx.clone());
                self.running = true;
                Some(tx)
            }
            Err(e) => {
                tracing::warn!("Failed to start CPAL stream: {}", e);
                None
            }
        }
    }

    /// Get a sender for pushing PCM samples.
    /// Returns `None` if the stream has not been started.
    pub fn sender(&self) -> Option<mpsc::Sender<f32>> {
        self.sender.clone()
    }

    /// Get the actual sample rate of the output stream.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Stop the output stream.
    pub fn stop(&mut self) {
        self.stream = None;
        self.sender = None;
        self.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl Drop for CpalOutput {
    fn drop(&mut self) {
        // Stream is dropped automatically, which stops playback.
        tracing::debug!("CpalOutput dropped, audio stream stopped");
    }
}

impl Default for CpalOutput {
    fn default() -> Self {
        Self::new()
    }
}
