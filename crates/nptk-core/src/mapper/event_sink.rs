//! CartridgeEventSink trait — Mapper 向 Cartridge 发送事件的接口
//!
//! Mapper 通过此接口通知 Cartridge/Runtime 发生的事件，如 IRQ 触发、
//! 调试跟踪等。

/// 卡带事件接收器
pub trait CartridgeEventSink {
    /// 设置 IRQ 线（Mapper 触发 IRQ）
    fn set_irq(&mut self);

    /// 清除 IRQ 线
    fn clear_irq(&mut self);

    /// 调试跟踪消息（默认实现为空）
    fn trace(&mut self, _msg: &str) {}
}

/// 默认事件接收器（空实现）
pub struct NullEventSink;

impl CartridgeEventSink for NullEventSink {
    fn set_irq(&mut self) {}
    fn clear_irq(&mut self) {}
}
