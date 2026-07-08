//! 音频桥接 — 连接 APU 与 CPAL/Kira 输出

use crate::runtime::AudioEventSink;
use std::sync::mpsc;

pub struct AudioBridge {
    sample_tx: Option<mpsc::Sender<f32>>,
    buffer: Vec<f32>,
}

impl AudioBridge {
    pub fn new() -> Self {
        AudioBridge {
            sample_tx: None,
            buffer: Vec::new(),
        }
    }

    pub fn set_output(&mut self, tx: mpsc::Sender<f32>) {
        self.sample_tx = Some(tx);
    }

    pub fn drain(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.buffer)
    }
}

impl AudioEventSink for AudioBridge {
    fn push_sample(&mut self, sample: f32) {
        self.buffer.push(sample);
        if let Some(ref tx) = self.sample_tx {
            let _ = tx.send(sample);
        }
    }
}

impl Default for AudioBridge {
    fn default() -> Self {
        Self::new()
    }
}
