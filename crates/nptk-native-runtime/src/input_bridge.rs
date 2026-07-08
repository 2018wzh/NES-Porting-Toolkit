//! 输入桥接 — 连接 nptk-input 系统与 NES 控制器端口

use nptk_core::controller::NesControllerState;

pub struct InputBridge {
    pub port1: NesControllerState,
    pub port2: NesControllerState,
}

impl InputBridge {
    pub fn new() -> Self {
        InputBridge {
            port1: NesControllerState::default(),
            port2: NesControllerState::default(),
        }
    }

    pub fn set_port1(&mut self, state: NesControllerState) {
        self.port1 = state;
    }
    pub fn set_port2(&mut self, state: NesControllerState) {
        self.port2 = state;
    }
}

impl Default for InputBridge {
    fn default() -> Self {
        Self::new()
    }
}
