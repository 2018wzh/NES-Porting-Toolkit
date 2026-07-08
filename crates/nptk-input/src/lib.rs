//! nptk-input: 可插拔输入系统
//! 跨平台输入后端：键盘、gamepad (gilrs)、通用 HID

pub mod backend;
pub mod canonical;
pub mod hotplug;
pub mod mapper;
pub mod nes_controller;
pub mod replay;

pub mod backends {
    //! Concrete input backend implementations
    pub mod gilrs_gamepad;
    pub mod hidapi_generic;
    pub mod winit_keyboard;
}
