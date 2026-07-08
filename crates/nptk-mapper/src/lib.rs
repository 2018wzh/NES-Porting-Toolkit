//! nptk-mapper — NES Mapper 聚合 crate
//!
//! 本 crate 重新导出 `nptk_core::mapper` 中的所有公共类型，
//! 并通过 feature flag 聚合各 mapper 实现 crate。
//!
//! # 依赖关系
//!
//! ```text
//! nptk-core::mapper::registry (全局注册表)
//!   ↑
//! nptk-mapper (init() 中注册所有启用的 mapper)
//!   ↑
//! mapper-nrom, mapper-uxrom, mapper-cnrom (提供构造器)
//! ```
//!
//! # Feature flags
//!
//! - `nrom` — 启用 NROM (Mapper 0) 实现（默认启用）
//! - `uxrom` — 启用 UxROM (Mapper 2) 实现（默认启用）
//! - `cnrom` — 启用 CNROM (Mapper 3) 实现（默认启用）

// 重新导出 nptk_core::mapper 中的所有公共类型
pub use nptk_core::mapper::*;

/// 便捷 prelude 模块
pub mod prelude {
    pub use nptk_core::mapper::{
        AddressMapper, Cartridge, CartridgeEventSink, MapperChip, MapperContext,
    };
}

// ── Feature-gated mapper 实现 ──
//
// 各 mapper crate 通过 init() 函数显式注册到全局注册表。

#[cfg(feature = "cnrom")]
use mapper_cnrom::Mapper003Cnrom;
#[cfg(feature = "nrom")]
use mapper_nrom::Mapper000Nrom;
#[cfg(feature = "uxrom")]
use mapper_uxrom::Mapper002Uxrom;

/// 初始化 mapper 注册表
///
/// 必须在首次调用 `create_mapper()` 之前调用。
/// 通常在程序入口处调用一次即可。
///
/// 根据启用的 feature flag，注册对应的 mapper 实现。
pub fn init() {
    use nptk_core::mapper::registry;

    #[cfg(feature = "nrom")]
    {
        static NROM: registry::MapperConstructor = registry::MapperConstructor {
            mapper_id: 0,
            name: "NROM",
            construct: |rom| Box::new(Mapper000Nrom::new(rom)),
        };
        registry::register_mapper(&NROM);
    }

    #[cfg(feature = "uxrom")]
    {
        static UXROM: registry::MapperConstructor = registry::MapperConstructor {
            mapper_id: 2,
            name: "UxROM",
            construct: |rom| Box::new(Mapper002Uxrom::new(rom)),
        };
        registry::register_mapper(&UXROM);
    }

    #[cfg(feature = "cnrom")]
    {
        static CNROM: registry::MapperConstructor = registry::MapperConstructor {
            mapper_id: 3,
            name: "CNROM",
            construct: |rom| Box::new(Mapper003Cnrom::new(rom)),
        };
        registry::register_mapper(&CNROM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_and_create() {
        // 先初始化
        init();

        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        let rom = nptk_core::rom::parse_rom(&data).unwrap();

        // NROM (mapper 0) 应该可用
        let mapper = registry::create_mapper(0, &rom);
        assert!(mapper.is_some(), "NROM should be registered after init");
        if let Some(m) = mapper {
            assert_eq!(m.mapper_id(), 0);
        }
    }
}
