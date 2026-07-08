//! AOT 绑定代码生成器
//!
//! 为 Cranelift 编译的静态链接块生成 Rust 绑定代码。
//! 绑定代码包含 `extern "C"` 函数声明和 dispatch 表，
//! 在编译期从 .a 静态库中解析符号。
//!
//! 实际静态链接流程在 `games/battle-city/build.rs` 中完成：
//!   Cranelift .o → ar → .a → cargo:rustc-link-lib=static

use std::collections::HashMap;

/// 生成的绑定代码
pub struct GeneratedBindings {
    /// Rust 源码字符串
    pub rust_source: String,
    /// 块地址 → 函数名映射
    pub block_map: HashMap<u16, String>,
}

/// 生成 Rust 绑定代码（静态链接风格）
///
/// 生成的代码包含:
/// - 每个块的 `extern "C"` 函数声明
/// - `get_dispatch()` 函数，返回 `Vec<(u16, CAbiBlockFn)>`
pub fn generate_bindings(blocks: &[crate::codegen::CompiledBlock]) -> GeneratedBindings {
    let mut src = String::new();

    src.push_str("//! Auto-generated bindings for recompiled NES blocks\n");
    src.push_str("//! Statically linked via build.rs\n\n");

    src.push_str("use nptk_native_runtime::runtime::CAbiBlockFn;\n");
    src.push_str("use nptk_native_runtime::runtime::NativeCpuState;\n");
    src.push_str("use nptk_core::bus::NesBusImpl;\n\n");

    // 为每个块生成 extern "C" 声明
    src.push_str("// ── Extern function declarations (from Cranelift AOT .a) ──\n");
    for block in blocks {
        src.push_str(&format!(
            "unsafe extern \"C\" {{ pub fn {}(bus: *mut NesBusImpl, cpu: *mut NativeCpuState) -> u32; }}\n",
            block.name
        ));
    }

    // 生成 dispatch 表
    src.push_str("\n/// Get the dispatch table of all compiled blocks\n");
    src.push_str("pub fn get_dispatch() -> Vec<(u16, CAbiBlockFn)> {\n");
    src.push_str("    vec![\n");
    for block in blocks {
        src.push_str(&format!(
            "        (0x{:04X}, {} as CAbiBlockFn),\n",
            block.address, block.name
        ));
    }
    src.push_str("    ]\n");
    src.push_str("}\n");

    GeneratedBindings {
        rust_source: src,
        block_map: blocks.iter().map(|b| (b.address, b.name.clone())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_bindings() {
        let blocks = vec![crate::codegen::CompiledBlock {
            address: 0x8000,
            name: "block_8000".to_string(),
        }];
        let bindings = generate_bindings(&blocks);
        assert!(bindings.rust_source.contains("CAbiBlockFn"));
        assert!(bindings.rust_source.contains("get_dispatch"));
    }
}
