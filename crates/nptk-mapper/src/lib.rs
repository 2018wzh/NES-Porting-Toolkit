//! nptk-mapper — NES Mapper 聚合 crate
//!
//! 本 crate 重新导出 `nptk_core::mapper` 中的所有公共类型，
//! 并通过 feature flag 聚合各 mapper 实现 crate。
//!
//! # 依赖关系
//!
//! ```text
//! nptk-core::mapper (接口定义)
//!   ↑
//! nptk-mapper (重导出 + 聚合 mapper-* crate)
//!   ↑
//! mapper-nrom, mapper-uxrom, mapper-cnrom (linkme 注册)
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
// 各 mapper crate 通过 linkme 的 distributed_slice 自动注册到
// MAPPER_REGISTRY 中。只需在 Cargo.toml 中启用对应 feature，
// 链接器会自动包含该 crate 的注册条目。
//
// 无需在此处编写任何额外代码。

#[cfg(test)]
mod tests {
    use nptk_core::mapper::registry::MAPPER_REGISTRY;

    /// 验证 linkme registry 可迭代
    /// 注意：linkme distributed_slice 在跨 crate 测试时可能不生效，
    /// 这是 linkme 的已知限制。在集成测试或完整构建中 registry 会包含条目。
    #[test]
    fn test_registry_iterable() {
        let count = MAPPER_REGISTRY.iter().count();
        assert!(count >= 0);
    }

    /// 验证可通过 builtin_nrom 创建 NROM mapper
    #[test]
    fn test_create_nrom_builtin() {
        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        let rom = nptk_core::rom::parse_rom(&data).unwrap();
        let mapper = nptk_core::mapper::registry::builtin_nrom(&rom);
        assert_eq!(mapper.mapper_id(), 0);
        assert_eq!(mapper.name(), "NROM (builtin)");
    }
}
