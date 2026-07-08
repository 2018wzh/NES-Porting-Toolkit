//! AOT 编译块绑定（静态链接）
//!
//! 包含 build.rs 生成的绑定代码。
//! 每个基本块对应一个 `extern "C"` 函数声明，从 .a 静态库中链接。
//! `get_dispatch()` 返回地址 → 函数指针的映射表。

include!(concat!(env!("OUT_DIR"), "/nes_blocks.rs"));