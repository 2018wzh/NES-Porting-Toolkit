//! Mapper 抽象与实现

use crate::rom::Mirroring;
use std::boxed::Box;

pub mod nrom;

/// Mapper trait — 所有 mapper 需实现此接口
pub trait Mapper {
    fn cpu_read(&mut self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, value: u8) -> bool;
    fn ppu_read(&mut self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, value: u8) -> bool;
    fn mirroring(&self) -> Mirroring;
    fn mapper_id(&self) -> u16;
}

/// 根据 mapper ID 创建 mapper 实例
pub fn create_mapper(mapper_id: u16, rom: &crate::rom::NesRom) -> Option<Box<dyn Mapper>> {
    match mapper_id {
        0 => Some(Box::new(nrom::Mapper0Nrom::new(rom))),
        _ => None,
    }
}
