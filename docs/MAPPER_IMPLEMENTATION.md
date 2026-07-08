# NES Mapper 实现指南

## 架构概述

```
nptk-core::mapper (接口定义)
  ├── MapperChip trait      — 所有 mapper 需实现的核心接口
  ├── MapperContext          — mapper 运行上下文 (Rc<RefCell<>>)
  ├── Cartridge              — 卡带容器，封装 mapper + 存储
  ├── AddressMapper          — 地址翻译辅助 trait
  ├── CartridgeEventSink     — 事件接收器 (IRQ 等)
  ├── ExpansionAudio         — 扩展音频接口
  └── registry               — linkme 分布式注册机制
        ↑
nptk-mapper (聚合 crate，重导出 + 聚合 mapper-*)
        ↑
mappers/nrom/   (独立 crate，linkme 注册)
mappers/uxrom/  (独立 crate，linkme 注册)
mappers/cnrom/  (独立 crate，linkme 注册)
```

### 依赖关系

- **nptk-core::mapper** — 定义所有接口和类型，不依赖任何 mapper 实现 crate
- **nptk-mapper** — 轻量聚合 crate，依赖 nptk-core + 所有 mapper-* crate，提供便捷的 `create_mapper()` 工厂函数
- **mapper-nrom/mapper-uxrom/mapper-cnrom** — 独立实现 crate，依赖 nptk-core，通过 linkme 注册

### 注册机制

使用 [linkme](https://crates.io/crates/linkme) 的 `distributed_slice` 实现分布式注册。
每个 mapper 实现 crate 在编译时自动将其构造器注册到全局 `MAPPER_REGISTRY` 中。

```rust
use nptk_core::mapper::registry::{MapperConstructor, MAPPER_REGISTRY};

#[linkme::distributed_slice(MAPPER_REGISTRY)]
static MY_MAPPER: MapperConstructor = MapperConstructor {
    mapper_id: 123,
    name: "MyMapper",
    construct: |rom| Box::new(MyMapper::new(rom)),
};
```

## 如何实现一个自定义 Mapper

### 步骤 1：创建 crate

在 `mappers/` 目录下创建新的 crate：

```toml
# mappers/my-mapper/Cargo.toml
[package]
name = "mapper-my-mapper"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
nptk-core = { path = "../../crates/nptk-core" }
linkme.workspace = true
serde_json.workspace = true   # 用于 save_state/load_state
```

在 workspace 根 `Cargo.toml` 中添加：

```toml
members = [
    # ... 现有成员 ...
    "mappers/my-mapper",
]
```

### 步骤 2：实现 MapperChip trait

```rust
// mappers/my-mapper/src/lib.rs
use std::cell::RefCell;
use std::rc::Rc;

use nptk_core::mapper::registry::{MapperConstructor, MAPPER_REGISTRY};
use nptk_core::mapper::types::{
    IrqState, MapperDebugInfo, MapperSaveState, PpuBusEvent,
};
use nptk_core::mapper::{MapperChip, MapperContext};
use nptk_core::rom::{Mirroring, NesRom};

pub struct MyMapper {
    // Mapper 内部状态
    some_register: u8,
    mirroring: Mirroring,
    irq_state: IrqState,
}

impl MyMapper {
    pub fn new(rom: &NesRom) -> Self {
        MyMapper {
            some_register: 0,
            mirroring: rom.header.mirroring,
            irq_state: IrqState::Inactive,
        }
    }
}

impl MapperChip for MyMapper {
    fn mapper_id(&self) -> u16 { 123 }
    fn name(&self) -> &'static str { "MyMapper" }

    // ── CPU 总线 ──

    fn cpu_read(
        &mut self,
        ctx: &Rc<RefCell<MapperContext>>,
        addr: u16,
    ) -> Option<u8> {
        // 处理 CPU 读 $4020-$FFFF 范围
        match addr {
            0x8000..=0xFFFF => {
                let ctx = ctx.borrow();
                let prg = &ctx.prg_rom;
                // ... 地址映射逻辑 ...
                Some(prg[offset])
            }
            _ => None, // 未映射的地址
        }
    }

    fn cpu_write(
        &mut self,
        ctx: &Rc<RefCell<MapperContext>>,
        addr: u16,
        value: u8,
    ) -> bool {
        // 处理 CPU 写 $4020-$FFFF 范围
        match addr {
            0x8000..=0xFFFF => {
                // 更新内部寄存器
                self.some_register = value;
                true // 表示已处理
            }
            _ => false,
        }
    }

    // ── PPU 总线 ──

    fn ppu_read(
        &mut self,
        ctx: &Rc<RefCell<MapperContext>>,
        addr: u16,
    ) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => {
                let ctx = ctx.borrow();
                Some(ctx.chr.read(addr))
            }
            _ => None,
        }
    }

    fn ppu_write(
        &mut self,
        ctx: &Rc<RefCell<MapperContext>>,
        addr: u16,
        value: u8,
    ) -> bool {
        match addr {
            0x0000..=0x1FFF => {
                let mut ctx = ctx.borrow_mut();
                ctx.chr.write(addr, value)
            }
            _ => false,
        }
    }

    // ── 时钟推进 ──

    fn cpu_tick(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _cycles: u32) {
        // 某些 mapper 需要 CPU 周期计数（如 MMC3 的 IRQ 计数器）
    }

    fn ppu_tick(
        &mut self,
        _ctx: &Rc<RefCell<MapperContext>>,
        _event: PpuBusEvent,
    ) {
        // 某些 mapper 需要观察 PPU 地址线（如 MMC3 的 A12 上升沿检测）
    }

    // ── IRQ ──

    fn irq_state(&self) -> IrqState {
        self.irq_state
    }

    fn clear_irq(&mut self) {
        self.irq_state = IrqState::Inactive;
    }

    // ── 镜像 ──

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    // ── 状态持久化 ──

    fn save_state(&self) -> MapperSaveState {
        let data = serde_json::json!({
            "some_register": self.some_register,
        });
        MapperSaveState {
            mapper_id: 123,
            data,
        }
    }

    fn load_state(&mut self, state: &MapperSaveState) {
        if let Some(v) = state.data.get("some_register").and_then(|v| v.as_u64()) {
            self.some_register = v as u8;
        }
    }

    // ── 调试（可选）──

    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo {
            registers: vec![
                ("Some Register".into(), format!("0x{:02X}", self.some_register)),
            ],
            ..Default::default()
        }
    }
}
```

### 步骤 3：通过 linkme 注册

```rust
#[linkme::distributed_slice(MAPPER_REGISTRY)]
static MY_MAPPER: MapperConstructor = MapperConstructor {
    mapper_id: 123,
    name: "MyMapper",
    construct: |rom| Box::new(MyMapper::new(rom)),
};
```

### 步骤 4：在 nptk-mapper 中添加 feature（可选）

如果希望 `nptk-mapper` 聚合 crate 默认包含你的 mapper，在 `crates/nptk-mapper/Cargo.toml` 中添加：

```toml
[dependencies]
# ... 现有依赖 ...
mapper-my-mapper = { path = "../../mappers/my-mapper", optional = true }

[features]
default = ["nrom", "uxrom", "cnrom", "my-mapper"]
my-mapper = ["dep:mapper-my-mapper"]
```

## MapperContext 使用指南

`MapperContext` 通过 `Rc<RefCell<>>` 包装，Mapper 方法接收 `&Rc<RefCell<MapperContext>>`。

### 读取数据（不可变借用）

```rust
fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
    let ctx = ctx.borrow(); // 不可变借用
    let value = ctx.prg_rom[offset];
    // ctx 在作用域结束时自动释放
    Some(value)
}
```

### 写入数据（可变借用）

```rust
fn cpu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
    let mut ctx = ctx.borrow_mut(); // 可变借用
    ctx.chr.write(addr, value);
    true
    // ctx 在作用域结束时自动释放
}
```

### 触发 IRQ

```rust
fn cpu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
    // 更新内部状态...
    self.irq_state = IrqState::Active;

    // 通知 Cartridge/Runtime
    let mut ctx = ctx.borrow_mut();
    ctx.event_sink.set_irq();
    true
}
```

### 重要：避免死锁

`RefCell` 在同一个作用域内不能同时存在可变和不可变借用：

```rust
// ❌ 错误：同时可变和不可变借用
let mut ctx = ctx.borrow_mut();
let val = ctx.prg_rom[0]; // 不可变借用（通过 prg_rom）
ctx.chr.write(addr, value); // 可变借用

// ✅ 正确：分开作用域
let val = {
    let ctx = ctx.borrow();
    ctx.prg_rom[0]
};
let mut ctx = ctx.borrow_mut();
ctx.chr.write(addr, value);
```

## 参考实现

### NROM (Mapper 0)

最简单的 mapper，无 bank switching。
- 16KB PRG: `$8000-$BFFF` = first 16KB, `$C000-$FFFF` = same (mirrored)
- 32KB PRG: `$8000-$FFFF` 直接映射
- CHR: 直接映射 (CHR-ROM 或 CHR-RAM)

完整代码见 `mappers/nrom/src/lib.rs`

### UxROM (Mapper 2)

16KB PRG bank switching。
- `$8000-$BFFF`: 可切换的 16KB PRG bank
- `$C000-$FFFF`: 固定到最后一个 16KB PRG bank
- CPU 写 `$8000-$FFFF`: 选择 PRG bank（低 4 位有效）

完整代码见 `mappers/uxrom/src/lib.rs`

### CNROM (Mapper 3)

8KB CHR bank switching。
- CPU `$8000-$FFFF`: 写入值选择 CHR bank（低 2 位有效）
- PRG: 固定 32KB PRG-ROM
- CHR: 通过 bank 选择切换 8KB CHR-ROM bank

完整代码见 `mappers/cnrom/src/lib.rs`

## 测试指南

### 单元测试

每个 mapper crate 应包含单元测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nptk_core::mapper::context::MapperContext;
    use nptk_core::mapper::event_sink::NullEventSink;
    use nptk_core::mapper::types::ChrStorage;
    use nptk_core::rom::parse_rom;

    fn make_test_rom() -> NesRom {
        let mut data = vec![0u8; 16 + 32768 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 2; // 2 × 16KB PRG
        data[5] = 1; // 1 × 8KB CHR
        parse_rom(&data).unwrap()
    }

    #[test]
    fn test_mapper_id() {
        let rom = make_test_rom();
        let mapper = MyMapper::new(&rom);
        assert_eq!(mapper.mapper_id(), 123);
    }

    #[test]
    fn test_cpu_read_prg() {
        let rom = make_test_rom();
        let mut mapper = MyMapper::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.unwrap_or_default()),
            Box::new(NullEventSink),
        ).into_rc();
        let val = mapper.cpu_read(&ctx, 0x8000);
        assert!(val.is_some());
    }

    #[test]
    fn test_linkme_registration() {
        let found = MAPPER_REGISTRY.iter().any(|e| e.mapper_id == 123);
        assert!(found, "MyMapper should be registered");
    }
}
```

### linkme 注册测试说明

linkme 的 `distributed_slice` 在跨 crate 测试时可能不生效（这是 linkme 的已知限制）。
在单独测试 mapper crate 时，`test_linkme_registration` 应该通过。
在测试依赖方（如 nptk-core 单独测试）时，registry 可能为空，应使用 `builtin_nrom` 回退。

## 调试技巧

### 启用 tracing 日志

```rust
fn cpu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
    tracing::debug!("MyMapper::cpu_write(addr=0x{addr:04X}, value=0x{value:02X})");
    // ...
}
```

### 使用 debug_info

实现 `debug_info()` 方法后，可以通过 `Cartridge::debug_info()` 获取 mapper 内部状态，
用于调试 UI 显示。

### 使用 event_sink.trace

```rust
let mut ctx = ctx.borrow_mut();
ctx.event_sink.trace(&format!("Bank switched to {}", bank));
```

## 常见问题

### Q: 为什么 create_mapper 返回 None？

A: linkme 注册需要 mapper crate 被实际链接。确保：
1. mapper crate 在 `Cargo.toml` 中被添加为依赖
2. 在完整构建（非单 crate 测试）中使用 `nptk-mapper` 聚合 crate
3. 单 crate 测试时使用 `builtin_nrom()` 回退

### Q: 如何添加新的 mapper 类型？

A: 按照本指南的步骤操作：
1. 在 `mappers/` 下创建新 crate
2. 实现 `MapperChip` trait
3. 通过 linkme 注册
4. 在 `nptk-mapper` 中添加 feature（可选）
5. 在 workspace `Cargo.toml` 中添加成员
6. 编写测试并验证

### Q: Mapper 如何访问 PRG-RAM？

A: 通过 `ctx.borrow().prg_ram` 读取，`ctx.borrow_mut().prg_ram.write()` 写入。
PRG-RAM 的默认大小是 8KB（0x2000），可通过 `MapperContext::new()` 后的 `ctx.prg_ram` 调整。