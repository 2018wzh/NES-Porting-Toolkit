//! NES 控制器 (手柄) 实现
//!
//! 标准 NES 控制器通过 $4016 (端口 1) 和 $4017 (端口 2) 访问。
//! 使用移位寄存器读取按钮状态: 写入 strobe 锁存状态, 然后逐位移出。

/// NES 控制器状态 (8 个按钮)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NesControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl NesControllerState {
    /// 编码为移位寄存器的值 (A 为 bit 0, B 为 bit 1, ...)
    pub fn encode(&self) -> u8 {
        self.a as u8
            | (self.b as u8) << 1
            | (self.select as u8) << 2
            | (self.start as u8) << 3
            | (self.up as u8) << 4
            | (self.down as u8) << 5
            | (self.left as u8) << 6
            | (self.right as u8) << 7
    }

    /// 从移位寄存器值解码
    pub fn decode(value: u8) -> Self {
        NesControllerState {
            a: value & 0x01 != 0,
            b: value & 0x02 != 0,
            select: value & 0x04 != 0,
            start: value & 0x08 != 0,
            up: value & 0x10 != 0,
            down: value & 0x20 != 0,
            left: value & 0x40 != 0,
            right: value & 0x80 != 0,
        }
    }

    /// 处理相反方向 (Left+Right → neutral, Up+Down → neutral)
    pub fn sanitize_opposites(&self) -> Self {
        let mut s = *self;
        if s.left && s.right {
            s.left = false;
            s.right = false;
        }
        if s.up && s.down {
            s.up = false;
            s.down = false;
        }
        s
    }
}

/// NES 控制器端口 (带移位寄存器)
#[derive(Debug, Clone)]
pub struct NesControllerPort {
    /// 当前有效状态
    pub current: NesControllerState,
    /// 锁存的值 (移位寄存器内容)
    latched: u8,
    /// 当前移位索引
    shift_index: u8,
    /// strobe 标志
    strobe: bool,
}

impl NesControllerPort {
    pub fn new() -> Self {
        NesControllerPort {
            current: NesControllerState::default(),
            latched: 0,
            shift_index: 0,
            strobe: false,
        }
    }

    /// 从外部设置当前状态 (由输入系统调用)
    pub fn set_current(&mut self, state: NesControllerState) {
        self.current = state.sanitize_opposites();
    }

    /// 写入 $4016 strobe
    pub fn write_strobe(&mut self, value: u8) {
        let new_strobe = value & 0x01 != 0;
        if !self.strobe && new_strobe {
            // Rising edge — latch current state
            self.latched = self.current.encode();
            self.shift_index = 0;
        } else if self.strobe && !new_strobe {
            // Falling edge — start shifting
            self.shift_index = 0;
        }
        self.strobe = new_strobe;
    }

    /// 读取 $4016 (端口 1) 或 $4017 (端口 2)
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            // 持续输出 A 按钮状态
            return self.current.a as u8;
        }
        let bit = if self.shift_index < 8 {
            (self.latched >> self.shift_index) & 1
        } else {
            1 // 第 8 位以后返回 1 (open bus behavior)
        };
        self.shift_index = self.shift_index.saturating_add(1);
        bit
    }

    /// 复位
    pub fn reset(&mut self) {
        self.latched = 0;
        self.shift_index = 0;
        self.strobe = false;
        self.current = NesControllerState::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_shift_register() {
        let mut port = NesControllerPort::new();
        // Set buttons: A + B + Start
        port.set_current(NesControllerState {
            a: true, b: true, start: true, ..Default::default()
        });
        // Strobe to latch
        port.write_strobe(1);
        port.write_strobe(0);

        // Read in order: A, B, Select, Start, Up, Down, Left, Right
        assert_eq!(port.read(), 1); // A
        assert_eq!(port.read(), 1); // B
        assert_eq!(port.read(), 0); // Select
        assert_eq!(port.read(), 1); // Start
        assert_eq!(port.read(), 0); // Up
        assert_eq!(port.read(), 0); // Down
        assert_eq!(port.read(), 0); // Left
        assert_eq!(port.read(), 0); // Right
    }

    #[test]
    fn test_sanitize_opposites() {
        let state = NesControllerState {
            left: true, right: true, up: true, down: true, ..Default::default()
        };
        let sanitized = state.sanitize_opposites();
        assert!(!sanitized.left);
        assert!(!sanitized.right);
        assert!(!sanitized.up);
        assert!(!sanitized.down);
    }
}