//! NES APU 兼容实现
//!
//! 基本 NES APU:
//! - 寄存器 $4000-$4017
//! - 通道: Pulse 1, Pulse 2, Triangle, Noise, DMC
//! - 帧计数器 (~240Hz 步进)
//! - 简单采样混合

use std::vec::Vec;

/// APU 兼容实现
pub struct ApuCompat {
    // 寄存器
    regs: [u8; 0x18], // $4000-$4017

    // 帧计数器
    frame_counter: u32,
    frame_irq: bool,

    // 脉冲通道
    pulse1_duty: u8,
    pulse1_duty_idx: u8,
    pulse1_volume: u8,
    pulse1_freq: u16,
    pulse1_enabled: bool,
    pulse1_timer: u16,

    pulse2_duty: u8,
    pulse2_duty_idx: u8,
    pulse2_volume: u8,
    pulse2_freq: u16,
    pulse2_enabled: bool,
    pulse2_timer: u16,

    // 三角波
    triangle_linear: u8,
    triangle_freq: u16,
    triangle_enabled: bool,
    triangle_timer: u16,
    triangle_seq: u8,
    triangle_counter: u8,

    // 噪声
    noise_volume: u8,
    noise_freq: u16,
    noise_enabled: bool,
    noise_timer: u16,
    noise_shift: u16,

    // DMC (Delta Modulation Channel)
    dmc_enabled: bool,
    dmc_irq_enable: bool,
    dmc_loop: bool,
    dmc_rate_idx: u8,
    dmc_output: u8,
    dmc_sample_addr: u16,
    dmc_sample_len: u16,
    dmc_bytes_remaining: u16,
    dmc_timer: u16,
    dmc_current_addr: u16,
    #[allow(dead_code)]
    dmc_silence: bool,

    // 采样缓冲区
    samples: Vec<f32>,
    sample_timer: u16,
    #[allow(dead_code)]
    sample_rate_divider: u16,

    // 长度计数器 (简化)
    length_counters: [u8; 4], // pulse1, pulse2, triangle, noise

    // 包络
    envelope_divider: [u8; 4],
    envelope_counter: [u8; 4],
    envelope_start: [bool; 4],
}

impl ApuCompat {
    pub fn new() -> Self {
        ApuCompat {
            regs: [0; 0x18],
            frame_counter: 0,
            frame_irq: false,
            pulse1_duty: 0,
            pulse1_duty_idx: 0,
            pulse1_volume: 0,
            pulse1_freq: 0,
            pulse1_enabled: false,
            pulse1_timer: 0,
            pulse2_duty: 0,
            pulse2_duty_idx: 0,
            pulse2_volume: 0,
            pulse2_freq: 0,
            pulse2_enabled: false,
            pulse2_timer: 0,
            triangle_linear: 0,
            triangle_freq: 0,
            triangle_enabled: false,
            triangle_timer: 0,
            triangle_seq: 0,
            triangle_counter: 0,
            noise_volume: 0,
            noise_freq: 0,
            noise_enabled: false,
            noise_timer: 0,
            noise_shift: 1,
            dmc_enabled: false,
            dmc_irq_enable: false,
            dmc_loop: false,
            dmc_rate_idx: 0,
            dmc_output: 0,
            dmc_sample_addr: 0,
            dmc_sample_len: 0,
            dmc_bytes_remaining: 0,
            dmc_timer: 0,
            dmc_current_addr: 0,
            dmc_silence: true,
            samples: Vec::new(),
            sample_timer: 0,
            sample_rate_divider: 0,
            length_counters: [0; 4],
            envelope_divider: [0; 4],
            envelope_counter: [0; 4],
            envelope_start: [true; 4],
        }
    }

    /// 步进 APU
    pub fn step(&mut self, cycles: u32) {
        for _ in 0..cycles {
            self.tick();
        }
    }

    fn tick(&mut self) {
        // 帧计数器: CPU 每 29830 个周期触发一次 (~240Hz, 近似)
        self.frame_counter += 1;
        if self.frame_counter >= 29830 {
            self.frame_counter = 0;
            self.clock_frame();
        }

        // 通道步进 — 使用独立变量避免 self 的并发借用
        let (p1_timer, p1_duty_idx, p1_freq, p1_enabled, p1_duty) = (
            self.pulse1_timer,
            self.pulse1_duty_idx,
            self.pulse1_freq,
            self.pulse1_enabled,
            self.pulse1_duty,
        );
        let (new_t1, new_d1) =
            Self::step_pulse_val(p1_timer, p1_duty_idx, p1_freq, p1_enabled, p1_duty);
        self.pulse1_timer = new_t1;
        self.pulse1_duty_idx = new_d1;

        let (p2_timer, p2_duty_idx, p2_freq, p2_enabled, p2_duty) = (
            self.pulse2_timer,
            self.pulse2_duty_idx,
            self.pulse2_freq,
            self.pulse2_enabled,
            self.pulse2_duty,
        );
        let (new_t2, new_d2) =
            Self::step_pulse_val(p2_timer, p2_duty_idx, p2_freq, p2_enabled, p2_duty);
        self.pulse2_timer = new_t2;
        self.pulse2_duty_idx = new_d2;

        self.step_triangle();
        self.step_noise();
        self.step_dmc();

        // 采样输出
        self.sample_timer += 1;
        // 约 1789773 / 44100 ≈ 40.6 CPU 周期每采样 @ 44100Hz
        if self.sample_timer >= 40 {
            self.sample_timer = 0;
            let sample = self.mix();
            self.samples.push(sample);
        }
    }

    fn clock_frame(&mut self) {
        // Frame sequencer: 4-step mode (standard NES)
        // Steps: 0=quarter frame, 1=half frame, 2=quarter, 3=half+IRQ
        for i in 0..4 {
            self.clock_envelope(i);
        }

        // Quarter frame: clock envelope + triangle linear counter
        if self.triangle_enabled && self.triangle_counter > 0 {
            self.triangle_counter -= 1;
        }

        // Half frame: clock length counters
        for lc in self.length_counters.iter_mut() {
            if *lc > 0 {
                *lc -= 1;
            }
        }

        // Step 3: half frame + IRQ (unless disabled by $4017 bit 7)
        if self.regs[0x17] & 0x80 == 0 {
            self.frame_irq = true;
        }
    }

    fn clock_envelope(&mut self, channel: usize) {
        if self.envelope_start[channel] {
            self.envelope_counter[channel] = 15;
            self.envelope_divider[channel] = self.regs[channel * 4] & 0x0F;
            self.envelope_start[channel] = false;
        } else if self.envelope_divider[channel] > 0 {
            self.envelope_divider[channel] -= 1;
        } else {
            self.envelope_divider[channel] = self.regs[channel * 4] & 0x0F;
            if self.envelope_counter[channel] > 0 {
                self.envelope_counter[channel] -= 1;
            } else if (self.regs[channel * 4] & 0x20) != 0 {
                self.envelope_counter[channel] = 15;
            }
        }
    }

    fn get_envelope_volume(&self, channel: usize) -> u8 {
        if self.regs[channel * 4] & 0x10 != 0 {
            // Fixed volume
            self.regs[channel * 4] & 0x0F
        } else {
            self.envelope_counter[channel]
        }
    }

    #[allow(dead_code)]
    fn step_pulse(
        &mut self,
        timer: &mut u16,
        duty_idx: &mut u8,
        freq: u16,
        enabled: bool,
        duty: u8,
    ) {
        if !enabled || *timer == 0 {
            return;
        }
        *timer -= 1;
        if *timer == 0 {
            *timer = freq;
            *duty_idx = duty_idx.wrapping_add(1) & 0x07;
        }
        let _ = duty;
    }

    /// 纯函数版本的 pulse stepping（无 self 借用）
    fn step_pulse_val(timer: u16, duty_idx: u8, freq: u16, enabled: bool, _duty: u8) -> (u16, u8) {
        if !enabled || timer == 0 {
            return (timer, duty_idx);
        }
        let new_timer = timer - 1;
        if new_timer == 0 {
            (freq, duty_idx.wrapping_add(1) & 0x07)
        } else {
            (new_timer, duty_idx)
        }
    }

    fn step_triangle(&mut self) {
        if !self.triangle_enabled || self.triangle_timer == 0 {
            return;
        }
        self.triangle_timer -= 1;
        if self.triangle_timer == 0 {
            self.triangle_timer = self.triangle_freq;
            // Triangle wave sequence: 0-15 then 15-0
            if self.triangle_seq < 15 {
                self.triangle_seq += 1;
            } else if self.triangle_seq < 31 {
                self.triangle_seq -= 1;
            } else {
                self.triangle_seq = 0;
            }
        }
    }

    fn step_noise(&mut self) {
        if !self.noise_enabled || self.noise_timer == 0 {
            return;
        }
        self.noise_timer -= 1;
        if self.noise_timer == 0 {
            let freq_divider = match self.noise_freq {
                0 => 4,
                1 => 8,
                2 => 16,
                3 => 32,
                4 => 64,
                5 => 96,
                6 => 128,
                7 => 160,
                8 => 202,
                9 => 254,
                10 => 380,
                11 => 508,
                12 => 762,
                13 => 1016,
                14 => 2034,
                _ => 4068, // 15
            };
            self.noise_timer = freq_divider;

            let feedback = if self.regs[0x0C] & 0x80 != 0 {
                // Mode 1: use bit 6 (takes XOR with bit 1)
                ((self.noise_shift >> 6) ^ (self.noise_shift >> 1)) & 0x01
            } else {
                // Mode 0: use bit 1 (standard white noise)
                ((self.noise_shift >> 1) ^ (self.noise_shift >> 0)) & 0x01
            };
            self.noise_shift = (self.noise_shift >> 1) | (feedback << 14);
        }
    }

    /// Step the DMC channel.
    ///
    /// The DMC timer decrements each CPU cycle. When it reaches 0,
    /// the DMC outputs the current bit (MSB of the output buffer)
    /// and shifts the buffer. When the buffer is empty, it loads
    /// the next byte from memory via DMA.
    fn step_dmc(&mut self) {
        if !self.dmc_enabled {
            return;
        }

        // DMC rate table (NTSC): index → period in CPU cycles
        const DMC_RATES: [u16; 16] = [
            428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
        ];

        let rate = DMC_RATES[self.dmc_rate_idx as usize % 16];

        if self.dmc_timer == 0 {
            self.dmc_timer = rate;
        } else {
            self.dmc_timer -= 1;
        }

        if self.dmc_bytes_remaining > 0 {
            self.dmc_bytes_remaining -= 1;
            if self.dmc_bytes_remaining == 0 {
                if self.dmc_loop {
                    // Restart
                    self.dmc_current_addr = self.dmc_sample_addr;
                    self.dmc_bytes_remaining = self.dmc_sample_len;
                } else {
                    self.dmc_enabled = false;
                }
            }
        }
    }

    fn mix(&self) -> f32 {
        let pulse_out1 = if self.pulse1_enabled && self.length_counters[0] > 0 {
            self.pulse_sample(1)
        } else {
            0.0
        };
        let pulse_out2 = if self.pulse2_enabled && self.length_counters[1] > 0 {
            self.pulse_sample(2)
        } else {
            0.0
        };
        let triangle_out = if self.triangle_enabled && self.length_counters[2] > 0 {
            self.triangle_sample()
        } else {
            0.0
        };
        let noise_out = if self.noise_enabled && self.length_counters[3] > 0 {
            self.noise_sample()
        } else {
            0.0
        };
        let dmc_out = if self.dmc_enabled {
            self.dmc_output as f32 / 127.0
        } else {
            0.0
        };

        (pulse_out1 + pulse_out2 + triangle_out + noise_out + dmc_out) * 0.2
    }

    fn pulse_sample(&self, channel: u8) -> f32 {
        const DUTY_TABLE: [[u8; 8]; 4] = [
            [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
            [0, 1, 1, 0, 0, 0, 0, 0], // 25%
            [0, 1, 1, 1, 1, 0, 0, 0], // 50%
            [1, 0, 0, 1, 1, 1, 1, 1], // 25% (negated)
        ];

        let reg_base = if channel == 1 { 0x00 } else { 0x04 };
        let duty = (self.regs[reg_base] >> 6) as usize & 0x03;
        let duty_idx = if channel == 1 {
            self.pulse1_duty_idx
        } else {
            self.pulse2_duty_idx
        };

        if DUTY_TABLE[duty][duty_idx as usize] != 0 {
            let vol = if channel == 1 {
                self.get_envelope_volume(0)
            } else {
                self.get_envelope_volume(1)
            };
            vol as f32 / 15.0
        } else {
            0.0
        }
    }

    fn triangle_sample(&self) -> f32 {
        self.triangle_seq as f32 / 15.0 - 0.5
    }

    fn noise_sample(&self) -> f32 {
        if self.noise_shift & 0x01 == 0 {
            let vol = self.get_envelope_volume(3);
            vol as f32 / 15.0
        } else {
            0.0
        }
    }

    // ---- 寄存器接口 ----

    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                // Status register
                let mut status = 0u8;
                if self.length_counters[0] > 0 {
                    status |= 0x01;
                }
                if self.length_counters[1] > 0 {
                    status |= 0x02;
                }
                if self.length_counters[2] > 0 {
                    status |= 0x04;
                }
                if self.length_counters[3] > 0 {
                    status |= 0x08;
                }
                if self.dmc_enabled {
                    status |= 0x10;
                }
                if self.frame_irq {
                    status |= 0x40;
                }
                if self.frame_irq {
                    self.frame_irq = false; // clear on read
                }
                status
            }
            0x4017 => {
                // Frame counter — read returns open bus / debug value
                0
            }
            _ => {
                // Other registers
                let idx = (addr & 0x001F) as usize;
                if idx < self.regs.len() {
                    self.regs[idx]
                } else {
                    0
                }
            }
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        let idx = (addr & 0x001F) as usize;
        if idx < self.regs.len() {
            self.regs[idx] = value;
        }
        match addr {
            0x4000 => {
                // Pulse 1: DLC VPPP (Duty, Length/Halt, Constant/Envelope, Volume)
                self.pulse1_duty = (value >> 6) & 0x03;
                self.pulse1_volume = value & 0x0F;
                if value & 0x20 == 0 {
                    self.envelope_start[0] = true;
                }
            }
            0x4002 => {
                // Pulse 1: Frequency low
                self.pulse1_freq = (self.pulse1_freq & 0x0700) | value as u16;
            }
            0x4003 => {
                // Pulse 1: Frequency high + length counter
                self.pulse1_freq = (self.pulse1_freq & 0x00FF) | ((value as u16 & 0x07) << 8);
                self.pulse1_enabled = true;
                self.pulse1_duty_idx = 0;
                // Length counter load
                self.length_counters[0] = LENGTH_TABLE[(value >> 3) as usize];
                self.envelope_start[0] = true;
            }
            0x4004 => {
                self.pulse2_duty = (value >> 6) & 0x03;
                self.pulse2_volume = value & 0x0F;
                if value & 0x20 == 0 {
                    self.envelope_start[1] = true;
                }
            }
            0x4006 => {
                self.pulse2_freq = (self.pulse2_freq & 0x0700) | value as u16;
            }
            0x4007 => {
                self.pulse2_freq = (self.pulse2_freq & 0x00FF) | ((value as u16 & 0x07) << 8);
                self.pulse2_enabled = true;
                self.pulse2_duty_idx = 0;
                self.length_counters[1] = LENGTH_TABLE[(value >> 3) as usize];
                self.envelope_start[1] = true;
            }
            0x4008 => {
                // Triangle: CRRR RRRR (Control, Linear counter)
                self.triangle_linear = value & 0x7F;
                if value & 0x80 == 0 {
                    self.triangle_enabled = false;
                }
            }
            0x400A => {
                // Triangle: Frequency low
                self.triangle_freq = (self.triangle_freq & 0x0700) | value as u16;
            }
            0x400B => {
                // Triangle: Frequency high + length counter
                self.triangle_freq = (self.triangle_freq & 0x00FF) | ((value as u16 & 0x07) << 8);
                self.triangle_enabled = true;
                self.triangle_seq = 0;
                self.length_counters[2] = LENGTH_TABLE[(value >> 3) as usize];
            }
            0x400C => {
                // Noise: --L VPPP
                self.noise_volume = value & 0x0F;
                if value & 0x20 == 0 {
                    self.envelope_start[3] = true;
                }
            }
            0x400E => {
                // Noise: Frequency + mode
                self.noise_freq = (value & 0x0F) as u16;
            }
            0x400F => {
                // Noise: Length counter
                self.noise_enabled = true;
                self.length_counters[3] = LENGTH_TABLE[(value >> 3) as usize];
                self.envelope_start[3] = true;
            }
            0x4010 => {
                // DMC: IL-- RRRR (IRQ enable, Loop, Rate)
                self.dmc_irq_enable = value & 0x80 != 0;
                self.dmc_loop = value & 0x40 != 0;
                self.dmc_rate_idx = value & 0x0F;
            }
            0x4011 => {
                // DMC: -DDD DDDD (Direct load)
                self.dmc_output = value & 0x7F;
            }
            0x4012 => {
                // DMC: Sample address = $C000 + (A * 64)
                self.dmc_sample_addr = 0xC000u16 | ((value as u16) << 6);
            }
            0x4013 => {
                // DMC: Sample length = (L * 16) + 1
                self.dmc_sample_len = ((value as u16) << 4) | 1;
            }
            0x4015 => {
                // Status write — enable/disable channels
                if value & 0x01 == 0 {
                    self.length_counters[0] = 0;
                    self.pulse1_enabled = false;
                }
                if value & 0x02 == 0 {
                    self.length_counters[1] = 0;
                    self.pulse2_enabled = false;
                }
                if value & 0x04 == 0 {
                    self.length_counters[2] = 0;
                    self.triangle_enabled = false;
                }
                if value & 0x08 == 0 {
                    self.length_counters[3] = 0;
                    self.noise_enabled = false;
                }
                if value & 0x10 == 0 {
                    self.dmc_enabled = false;
                } else {
                    // Enable DMC: initialize address and remaining bytes
                    self.dmc_enabled = true;
                    self.dmc_current_addr = self.dmc_sample_addr;
                    self.dmc_bytes_remaining = self.dmc_sample_len;
                }
            }
            0x4017 => {
                // Frame counter control
                if value & 0x80 != 0 {
                    self.frame_counter = 0; // reset
                    // IRQ disable mode (5-step)
                    self.frame_irq = false;
                }
            }
            _ => {}
        }
    }

    /// 获取采样缓冲区 (清空内部缓冲区)
    pub fn get_samples(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.samples)
    }

    /// 当前 Pulse 1 通道输出值 (-1.0..1.0)
    pub fn pulse1_output(&self) -> f32 {
        if !self.pulse1_enabled || self.length_counters[0] == 0 {
            return 0.0;
        }
        let duty_table: [[u8; 8]; 4] = [
            [0, 1, 0, 0, 0, 0, 0, 0],
            [0, 1, 1, 0, 0, 0, 0, 0],
            [0, 1, 1, 1, 1, 0, 0, 0],
            [1, 0, 0, 1, 1, 1, 1, 1],
        ];
        let duty = self.pulse1_duty as usize % 4;
        let idx = self.pulse1_duty_idx as usize % 8;
        let val = duty_table[duty][idx] as f32;
        let vol = self.pulse1_volume.min(15) as f32 / 15.0;
        val * 2.0 * vol - 1.0
    }

    /// 当前 Pulse 2 通道输出值 (-1.0..1.0)
    pub fn pulse2_output(&self) -> f32 {
        if !self.pulse2_enabled || self.length_counters[1] == 0 {
            return 0.0;
        }
        let duty_table: [[u8; 8]; 4] = [
            [0, 1, 0, 0, 0, 0, 0, 0],
            [0, 1, 1, 0, 0, 0, 0, 0],
            [0, 1, 1, 1, 1, 0, 0, 0],
            [1, 0, 0, 1, 1, 1, 1, 1],
        ];
        let duty = self.pulse2_duty as usize % 4;
        let idx = self.pulse2_duty_idx as usize % 8;
        let val = duty_table[duty][idx] as f32;
        let vol = self.pulse2_volume.min(15) as f32 / 15.0;
        val * 2.0 * vol - 1.0
    }

    /// 当前 Triangle 通道输出值 (-1.0..1.0)
    pub fn triangle_output(&self) -> f32 {
        if !self.triangle_enabled || self.length_counters[2] == 0 {
            return 0.0;
        }
        let seq: [f32; 32] = [
            0.9375, 0.8125, 0.6875, 0.5625, 0.4375, 0.3125, 0.1875, 0.0625, -0.0625, -0.1875,
            -0.3125, -0.4375, -0.5625, -0.6875, -0.8125, -0.9375, -0.9375, -0.8125, -0.6875,
            -0.5625, -0.4375, -0.3125, -0.1875, -0.0625, 0.0625, 0.1875, 0.3125, 0.4375, 0.5625,
            0.6875, 0.8125, 0.9375,
        ];
        seq[self.triangle_seq as usize % 32]
    }

    /// 当前 Noise 通道输出值 (-1.0..1.0)
    pub fn noise_output(&self) -> f32 {
        if !self.noise_enabled || self.length_counters[3] == 0 {
            return 0.0;
        }
        let val = if (self.noise_shift & 1) != 0 {
            -1.0
        } else {
            1.0
        };
        let vol = self.noise_volume.min(15) as f32 / 15.0;
        val * vol
    }
}

/// NES APU 长度计数器表
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apu_initial_state() {
        let apu = ApuCompat::new();
        assert!(!apu.pulse1_enabled);
        assert!(!apu.pulse2_enabled);
        assert!(apu.samples.is_empty());
    }

    #[test]
    fn test_apu_status_read() {
        let mut apu = ApuCompat::new();
        let status = apu.read_register(0x4015);
        assert_eq!(status, 0); // all channels off
    }

    #[test]
    fn test_pulse_write() {
        let mut apu = ApuCompat::new();
        apu.write_register(0x4000, 0x30); // 50% duty, fixed volume=0
        apu.write_register(0x4002, 0x80); // freq low
        apu.write_register(0x4003, 0x08); // freq high=1 (>>3 = 1 → table[1]=254)
        assert!(apu.pulse1_enabled);
        assert_eq!(apu.length_counters[0], 254);
    }
}
