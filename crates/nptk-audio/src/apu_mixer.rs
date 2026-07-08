//! APU 兼容混音器
//! 将 APU 通道输出混合为 PCM 样本流

/// APU 混音器
pub struct ApuMixer {
    sample_rate: u32,
    cpu_clock: f64,
    cycle_accumulator: f64,
    samples: Vec<f32>,
    /// DC 阻塞滤波器: 一阶高通 IIR
    prev_input: f32,
    prev_output: f32,
}

impl ApuMixer {
    pub fn new(sample_rate: u32) -> Self {
        ApuMixer {
            sample_rate,
            cpu_clock: 1_789_773.0, // NTSC CPU clock
            cycle_accumulator: 0.0,
            samples: Vec::with_capacity(1024),
            prev_input: 0.0,
            prev_output: 0.0,
        }
    }

    /// 处理一个 APU 周期的输出
    /// pulse1, pulse2, triangle, noise: 各通道当前输出值 (-1.0..1.0)
    pub fn mix(&mut self, cycles: u32, pulse1: f32, pulse2: f32, triangle: f32, noise: f32) {
        let samples_per_cycle = self.sample_rate as f64 / self.cpu_clock;
        self.cycle_accumulator += cycles as f64 * samples_per_cycle;

        let count = self.cycle_accumulator.floor() as usize;
        self.cycle_accumulator -= count as f64;

        let mixed = self.mix_channels(pulse1, pulse2, triangle, noise);
        for _ in 0..count {
            // DC blocking filter: y[n] = 0.999 * y[n-1] + x[n] - x[n-1]
            let filtered = 0.999 * self.prev_output + mixed - self.prev_input;
            self.prev_input = mixed;
            self.prev_output = filtered;
            self.samples.push(filtered.clamp(-1.0, 1.0));
        }
    }

    fn mix_channels(&self, pulse1: f32, pulse2: f32, triangle: f32, noise: f32) -> f32 {
        // NES APU mixing formula
        let pulse_out = 0.00752 * (pulse1 + pulse2);
        let tnd_out = 0.00851 * triangle + 0.00494 * noise;
        (pulse_out + tnd_out).clamp(-1.0, 1.0)
    }

    /// 取出所有待播放样本
    pub fn drain_samples(&mut self) -> Vec<f32> {
        self.samples.drain(..).collect()
    }

    /// 清空缓冲区
    pub fn clear(&mut self) {
        self.samples.clear();
        self.cycle_accumulator = 0.0;
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }
}