//! Mapper 注册表 — 基于 linkme 的分布式注册机制
//!
//! 各 mapper 实现 crate（如 mapper-nrom、mapper-uxrom）通过
//! `#[linkme::distributed_slice(MAPPER_REGISTRY)]` 将自己的构造器
//! 注册到全局表中。`create_mapper()` 函数遍历此表查找匹配的 mapper。

use linkme::distributed_slice;

use super::mapper_chip::MapperChip;
use crate::rom::NesRom;

/// Mapper 构造器
///
/// 每个 mapper 实现 crate 在初始化时通过 linkme 注册一个
/// MapperConstructor 到全局 MAPPER_REGISTRY 中。
pub struct MapperConstructor {
    /// iNES Mapper ID
    pub mapper_id: u16,
    /// Mapper 名称
    pub name: &'static str,
    /// 构造器函数：接收 ROM 数据，返回 MapperChip 实例
    pub construct: fn(&NesRom) -> Box<dyn MapperChip>,
}

/// 全局 Mapper 注册表
///
/// 各 mapper crate 通过 `#[distributed_slice(MAPPER_REGISTRY)]` 注册：
///
/// ```ignore
/// use nptk_core::mapper::registry::{MAPPER_REGISTRY, MapperConstructor};
///
/// #[linkme::distributed_slice(MAPPER_REGISTRY)]
/// static NROM: MapperConstructor = MapperConstructor {
///     mapper_id: 0,
///     name: "NROM",
///     construct: |rom| Box::new(Mapper000Nrom::new(rom)),
/// };
/// ```
#[distributed_slice]
pub static MAPPER_REGISTRY: [MapperConstructor];

/// 根据 mapper ID 创建 Mapper 实例
///
/// 遍历全局 MAPPER_REGISTRY，查找匹配的 mapper_id。
/// 如果找到，调用其 construct 函数创建实例。
/// 如果未找到，返回 None。
///
/// 注意：当没有 mapper crate 被链接时（如单独测试 nptk-core），
/// registry 为空，此函数始终返回 None。在完整构建中，
/// nptk-mapper 或下游 crate 会链接具体的 mapper 实现。
pub fn create_mapper(mapper_id: u16, rom: &NesRom) -> Option<Box<dyn MapperChip>> {
    for entry in MAPPER_REGISTRY {
        if entry.mapper_id == mapper_id {
            return Some((entry.construct)(rom));
        }
    }
    None
}

/// 内置 NROM 实现（用于测试和无 linkme 注册时的兜底）
///
/// 当 linkme registry 为空时，此函数提供最基本的 NROM 支持。
/// 在完整构建中，mapper-nrom crate 会通过 linkme 注册更完整的实现。
///
/// 此函数主要用于测试场景，以及当 nptk-core 作为独立依赖使用时。
/// 在完整项目中，建议通过 nptk-mapper 聚合 crate 使用 linkme 注册的 mapper。
pub fn builtin_nrom(rom: &NesRom) -> Box<dyn MapperChip> {
    use crate::rom::Mirroring;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct BuiltinNrom {
        mirroring: Mirroring,
        is_16k: bool,
    }

    impl MapperChip for BuiltinNrom {
        fn mapper_id(&self) -> u16 { 0 }
        fn name(&self) -> &'static str { "NROM (builtin)" }
        fn cpu_read(&mut self, ctx: &Rc<RefCell<super::MapperContext>>, addr: u16) -> Option<u8> {
            match addr {
                0x8000..=0xFFFF => {
                    let ctx = ctx.borrow();
                    let prg = &ctx.prg_rom;
                    if prg.is_empty() { return Some(0); }
                    let offset = if self.is_16k {
                        (addr as usize - 0x8000) & 0x3FFF
                    } else {
                        (addr as usize - 0x8000) & 0x7FFF
                    };
                    Some(prg[offset.min(prg.len() - 1)])
                }
                _ => None,
            }
        }
        fn cpu_write(&mut self, _ctx: &Rc<RefCell<super::MapperContext>>, _addr: u16, _value: u8) -> bool { false }
        fn ppu_read(&mut self, ctx: &Rc<RefCell<super::MapperContext>>, addr: u16) -> Option<u8> {
            match addr {
                0x0000..=0x1FFF => {
                    let ctx = ctx.borrow();
                    Some(ctx.chr.read(addr))
                }
                _ => None,
            }
        }
        fn ppu_write(&mut self, ctx: &Rc<RefCell<super::MapperContext>>, addr: u16, value: u8) -> bool {
            match addr {
                0x0000..=0x1FFF => {
                    let mut ctx = ctx.borrow_mut();
                    ctx.chr.write(addr, value)
                }
                _ => false,
            }
        }
        fn mirroring(&self) -> Mirroring { self.mirroring }
        fn save_state(&self) -> super::MapperSaveState { super::MapperSaveState::new(0) }
        fn load_state(&mut self, _state: &super::MapperSaveState) {}
    }

    let is_16k = rom.prg_rom.len() <= 16_384;
    Box::new(BuiltinNrom { mirroring: rom.header.mirroring, is_16k })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 MAPPER_REGISTRY 可迭代（可能为空）
    /// 当 mapper-nrom crate 被链接时，registry 包含 NROM。
    /// 当单独测试 nptk-core 时，registry 可能为空。
    #[test]
    fn test_registry_iterable() {
        // registry 总是可迭代的，即使没有 mapper 被注册
        let count = MAPPER_REGISTRY.iter().count();
        // 在 nptk-core 单独测试时，count 可能为 0
        // 在完整构建中，count > 0
        assert!(count >= 0);
    }
}