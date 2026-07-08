//! MapperContext — Mapper 运行上下文
//!
//! Mapper 通过 `Rc<RefCell<MapperContext>>` 访问卡带存储和事件接口。
//! 所有 MapperChip 方法都接收 `&Rc<RefCell<MapperContext>>` 参数。

use std::cell::RefCell;
use std::rc::Rc;

use super::event_sink::CartridgeEventSink;
use super::types::{ChrStorage, NesRegion, PrgRam};

/// Mapper 运行上下文
///
/// 包含 Mapper 需要访问的所有卡带存储和运行环境信息。
/// 通过 `Rc<RefCell<>>` 包装，使得 Mapper 和 Cartridge 可以共享访问。
pub struct MapperContext {
    /// PRG-ROM 数据
    pub prg_rom: Vec<u8>,
    /// CHR 存储（ROM 或 RAM）
    pub chr: ChrStorage,
    /// PRG-RAM / SRAM
    pub prg_ram: PrgRam,
    /// Open bus 值
    pub open_bus: u8,
    /// NES 区域
    pub region: NesRegion,
    /// 事件接收器
    pub event_sink: Box<dyn CartridgeEventSink>,
}

impl MapperContext {
    /// 创建新的 MapperContext
    pub fn new(prg_rom: Vec<u8>, chr: ChrStorage, event_sink: Box<dyn CartridgeEventSink>) -> Self {
        MapperContext {
            prg_rom,
            chr,
            prg_ram: PrgRam::default(),
            open_bus: 0,
            region: NesRegion::Ntsc,
            event_sink,
        }
    }

    /// 包装为 Rc<RefCell<>> 以便共享
    pub fn into_rc(self) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(self))
    }
}
