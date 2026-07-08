//! Mapper 注册表 — 显式注册机制
//!
//! 使用 `OnceLock<Mutex<HashMap>>` 存储 mapper 构造器。
//! 各 mapper 实现 crate 通过 `nptk-mapper::init()` 统一注册，
//! 或通过 `register_mapper()` 单独注册。
//!
//! # 架构
//!
//! ```text
//! nptk-core::mapper::registry (接口定义 + 全局存储)
//!   ↑
//! nptk-mapper (init() 中注册所有启用的 mapper)
//!   ↑
//! mapper-nrom, mapper-uxrom, mapper-cnrom (提供构造器)
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::mapper_chip::MapperChip;
use crate::rom::NesRom;

/// Mapper 构造器
pub struct MapperConstructor {
    /// iNES Mapper ID
    pub mapper_id: u16,
    /// Mapper 名称
    pub name: &'static str,
    /// 构造器函数：接收 ROM 数据，返回 MapperChip 实例
    pub construct: fn(&NesRom) -> Box<dyn MapperChip>,
}

/// 全局 Mapper 注册表
fn global_registry() -> &'static Mutex<HashMap<u16, &'static MapperConstructor>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u16, &'static MapperConstructor>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 注册一个 mapper 构造器
///
/// 通常在 `nptk-mapper::init()` 中批量调用。
/// 重复注册同一 mapper_id 会覆盖旧值。
pub fn register_mapper(constructor: &'static MapperConstructor) {
    let mut reg = global_registry()
        .lock()
        .expect("Mapper registry lock poisoned");
    reg.insert(constructor.mapper_id, constructor);
}

/// 根据 mapper ID 创建 Mapper 实例
///
/// 在全局注册表中查找匹配的 mapper_id。
/// 如果找到，调用其 construct 函数创建实例。
/// 如果未找到，返回 None。
pub fn create_mapper(mapper_id: u16, rom: &NesRom) -> Option<Box<dyn MapperChip>> {
    let reg = global_registry()
        .lock()
        .expect("Mapper registry lock poisoned");
    reg.get(&mapper_id)
        .map(|constructor| (constructor.construct)(rom))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_create() {
        static TEST_MAPPER: MapperConstructor = MapperConstructor {
            mapper_id: 999,
            name: "TestMapper",
            construct: |_rom| {
                struct TestMapper;
                impl MapperChip for TestMapper {
                    fn mapper_id(&self) -> u16 { 999 }
                    fn name(&self) -> &'static str { "TestMapper" }
                    fn cpu_read(&mut self, _ctx: &std::rc::Rc<std::cell::RefCell<crate::mapper::MapperContext>>, _addr: u16) -> Option<u8> { None }
                    fn cpu_write(&mut self, _ctx: &std::rc::Rc<std::cell::RefCell<crate::mapper::MapperContext>>, _addr: u16, _value: u8) -> bool { false }
                    fn ppu_read(&mut self, _ctx: &std::rc::Rc<std::cell::RefCell<crate::mapper::MapperContext>>, _addr: u16) -> Option<u8> { None }
                    fn ppu_write(&mut self, _ctx: &std::rc::Rc<std::cell::RefCell<crate::mapper::MapperContext>>, _addr: u16, _value: u8) -> bool { false }
                    fn mirroring(&self) -> crate::rom::Mirroring { crate::rom::Mirroring::Horizontal }
                    fn save_state(&self) -> crate::mapper::MapperSaveState { crate::mapper::MapperSaveState::new(0) }
                    fn load_state(&mut self, _state: &crate::mapper::MapperSaveState) {}
                }
                Box::new(TestMapper)
            },
        };
        register_mapper(&TEST_MAPPER);
        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        let rom = crate::rom::parse_rom(&data).unwrap();
        let mapper = create_mapper(999, &rom);
        assert!(mapper.is_some());
        assert_eq!(mapper.unwrap().mapper_id(), 999);
    }
}
