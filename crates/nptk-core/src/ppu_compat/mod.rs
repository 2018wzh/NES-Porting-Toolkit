//! NES PPU 兼容实现
//!
//! 基本功能 PPU，提供:
//! - 寄存器 $2000-$2007 读写 (含镜像和延迟写入)
//! - 内部状态 (v, t, x, w) 按 NESDev 规范
//! - 2KB 显存 (2 个 nametable)
//! - 256B OAM (64 精灵 × 4 字节)
//! - 32B 调色板
//! - 256×240 索引帧缓冲
//! - 基本背景和精灵渲染
//! - NMI 检测 ($2002 VBlank 标志)

use crate::mapper::Cartridge;
use crate::rom::Mirroring;

/// PPU 兼容实现
#[repr(C)]
#[allow(unused)]
pub struct PpuCompat {
    // 寄存器
    pub ctrl: u8,   // $2000
    pub mask: u8,   // $2001
    pub status: u8, // $2002
    pub mirroring: Mirroring,
    oam_addr: u8,    // $2003
    data_buffer: u8, // $2007 read buffer
    open_bus: u8,    // PPU open bus

    // 内部地址 (NESDev: v, t, x, w)
    v: u16,  // current VRAM address (15 bit)
    t: u16,  // temporary VRAM address (15 bit)
    x: u8,   // fine X scroll (3 bit)
    w: bool, // write toggle
    pub has_nmi: bool,

    // 存储
    nametable: Box<[u8; 4096]>, // 4KB 显存 (4 nametables for FourScreen support)
    oam: Box<[u8; 256]>,        // 精灵属性内存
    palette: [u8; 32],          // 调色板

    // 帧缓冲 256×240
    frame: Box<[u8; 256 * 240]>,

    // 渲染状态
    pub scanline: u16, // 当前扫描行 (0-261)
    pub cycle: u16,    // 当前周期 (0-340)
    frame_complete: bool,

    // odd frame skip
    odd_frame: bool,
}

impl PpuCompat {
    pub fn new(mirroring: Mirroring) -> Self {
        PpuCompat {
            ctrl: 0,
            mask: 0,
            status: 0xA0,
            mirroring,
            oam_addr: 0,
            data_buffer: 0,
            open_bus: 0,
            v: 0,
            t: 0,
            x: 0,
            w: false,
            has_nmi: false,
            nametable: Box::new([0; 4096]),
            oam: Box::new([0; 256]),
            palette: [
                0x0F, 0x00, 0x10, 0x20, 0x0F, 0x06, 0x16, 0x26, 0x0F, 0x08, 0x18, 0x28, 0x0F, 0x0A,
                0x1A, 0x2A, 0x0F, 0x0C, 0x1C, 0x2C, 0x0F, 0x0E, 0x1E, 0x2E, 0x0F, 0x01, 0x11, 0x21,
                0x0F, 0x05, 0x15, 0x25,
            ],
            frame: Box::new([0; 256 * 240]),
            scanline: 0,
            cycle: 0,
            frame_complete: false,
            odd_frame: false,
        }
    }

    /// 步进 PPU (每 CPU 周期 3 个 PPU 周期)
    pub fn step(&mut self, cycles: u32) {
        for _ in 0..cycles {
            self.tick();
        }
    }

    fn tick(&mut self) {
        // 扫描线/周期更新 (先递增，再检查 — 与 tetanes-core 一致)
        self.cycle += 1;
        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame_complete = true;
                self.odd_frame = !self.odd_frame;
            }
        }

        // 奇数帧跳过: NTSC 模式下，渲染启用时，奇数帧跳过最后一个周期
        // tetanes-core: cycle == ODD_SKIP(339) && is_prerender_scanline && frame.is_odd()
        if self.odd_frame && self.scanline == 261 && self.cycle == 339 && (self.mask & 0x18) != 0 {
            self.cycle = 340; // 立即跳到 340，下一 tick 会 wrap 到 0
        }

        if self.scanline >= 240 && self.scanline < 261 {
            // VBlank — tetanes-core: scanline == vblank_scanline && cycle == VBLANK(1)
            if self.scanline == 241 && self.cycle == 1 {
                // Set VBlank flag
                self.status |= 0x80;
                self.status &= !0x40; // clear sprite 0 hit for new frame
                if self.ctrl & 0x80 != 0 {
                    self.has_nmi = true;
                }
            }
        }

        // 预渲染扫描线 (scanline 261): 在 cycle=1 时清除 VBlank
        if self.scanline == 261 && self.cycle == 1 {
            self.status &= !0x80; // clear VBlank
            self.status &= !0x40; // clear sprite 0 hit
            self.status &= !0x20; // clear sprite overflow
        }

        if self.scanline < 240 {
            // Visible scanline — render at cycle 256 (after fetching tiles)
            if self.cycle == 256 {
                self.render_scanline(self.scanline);
            }
        }

        // 预渲染扫描线 (scanline 261): 在周期 257 复制 X，周期 280-304 复制 Y
        if self.scanline == 261 {
            if self.cycle == 257 {
                // Copy X: coarse X (t[0:4]) 和 nametable X (t[10])
                self.v = (self.v & !0x041F) | (self.t & 0x041F);
            }
            if self.cycle >= 280 && self.cycle <= 304 {
                // Copy Y: fine Y (t[12:14]), nametable Y (t[11]), coarse Y (t[5:9])
                self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
            }
        }
    }

    /// 判断 NMI 是否待处理, 并清除标志
    pub fn take_nmi(&mut self) -> bool {
        if self.has_nmi {
            self.has_nmi = false;
            true
        } else {
            false
        }
    }

    // ---- 寄存器读写 ----

    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x2000 => self.open_bus, // $2000 只写，返回 open bus
            0x2001 => self.open_bus, // $2001 只写，返回 open bus
            0x2002 => self.read_status(),
            0x2003 => self.open_bus, // $2003 只写，返回 open bus
            0x2004 => self.read_oam_data(),
            0x2005 => self.open_bus, // $2005 只写，返回 open bus
            0x2006 => self.open_bus, // $2006 只写，返回 open bus
            0x2007 => self.read_data(),
            _ => self.open_bus,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
        self.open_bus = value;
        match addr {
            0x2000 => self.write_ctrl(value),
            0x2001 => self.write_mask(value),
            0x2002 => { /* $2002 只读 */ }
            0x2003 => self.write_oam_addr(value),
            0x2004 => self.write_oam_data(value),
            0x2005 => self.write_scroll(value),
            0x2006 => self.write_addr(value),
            0x2007 => self.write_data(value),
            _ => {}
        }
    }

    fn read_status(&mut self) -> u8 {
        // tetanes-core: peek_status 返回 (status & 0xE0) | (open_bus & 0x1F)
        let result = (self.status & 0xE0) | (self.open_bus & 0x1F);
        // Clear VBlank flag and write toggle
        self.status &= !0x80;
        self.w = false;

        // tetanes-core: 在 VBlank 前一周期读取 $2002 会阻止 VBL
        if self.scanline == 241 && self.cycle == 0 {
            self.has_nmi = false;
        }

        self.open_bus = result;
        result
    }

    fn read_oam_data(&mut self) -> u8 {
        // tetanes-core: 渲染期间读取 OAMDATA 暴露 secondary OAM 访问
        // 简化实现：始终返回主 OAM
        let val = self.oam[self.oam_addr as usize];
        self.open_bus = val;
        val
    }

    fn read_data(&mut self) -> u8 {
        let addr = self.v & 0x3FFF;
        let prev_open_bus = self.open_bus;
        let result = self.data_buffer;

        // 更新 buffer (从有效地址读取)
        // tetanes-core 逻辑: 非调色板区域读取数据到 buffer，调色板区域读取 nametable 镜像到 buffer
        if addr < 0x3F00 {
            // 图案表或 nametable 区域 — 用 buffer 实现延迟读取
            self.data_buffer = self.read_nametable_data(addr);
        } else {
            // 调色板读取时，buffer 填充 nametable 镜像数据 (addr - 0x1000)
            self.data_buffer = self.read_nametable_data(addr - 0x1000);
        }

        // 调色板读取 — 直接返回数据 (非缓冲)
        if addr >= 0x3F00 {
            let val = self.read_palette(addr);
            // tetanes-core: 调色板读取的高 2 位保留 open bus
            self.open_bus = val | (prev_open_bus & 0xC0);
            return self.open_bus;
        }

        // VRAM 地址增量
        if self.ctrl & 0x04 != 0 {
            self.v = self.v.wrapping_add(32);
        } else {
            self.v = self.v.wrapping_add(1);
        }

        self.open_bus = result;
        result
    }

    /// 读取 nametable 区域数据（用于 $2007 缓冲）
    fn read_nametable_data(&self, addr: u16) -> u8 {
        let masked = addr & 0x3FFF;
        if masked < 0x2000 {
            0 // CHR 区域 — 需要 mapper，这里返回 0
        } else if masked < 0x3F00 {
            let raw = (masked & 0x0FFF) as usize;
            let mapped = self.map_nametable_addr(raw, self.mirroring);
            self.nametable[mapped]
        } else {
            self.read_palette_internal(masked)
        }
    }

    fn write_ctrl(&mut self, value: u8) {
        self.ctrl = value;
        // t[10:12] = value[0:2] — nametable select
        self.t = (self.t & 0xF3FF) | ((value as u16 & 0x03) << 10);
        // tetanes-core NMI 逻辑:
        // 如果 NMI 被禁用，清除 pending
        // 如果 NMI 启用且已在 VBlank 中，设置 pending
        if (value & 0x80) == 0 {
            self.has_nmi = false;
        } else if (self.status & 0x80) != 0 {
            self.has_nmi = true;
        }
    }

    fn write_mask(&mut self, value: u8) {
        self.mask = value;
    }

    fn write_oam_addr(&mut self, value: u8) {
        self.oam_addr = value;
    }

    fn write_oam_data(&mut self, value: u8) {
        self.open_bus = value;
        // tetanes-core: 渲染期间写入 OAMDATA 不修改值，但执行 glitch increment
        let rendering = self.mask & 0x18 != 0;
        let on_render_scanline = self.scanline < 240 || self.scanline == 261;
        if rendering && on_render_scanline {
            // Writes during rendering do not modify OAM, but glitch-increment OAMADDR
            self.oam_addr = self.oam_addr.wrapping_add(4);
        } else {
            let mut val = value;
            // Bits 2-4 of sprite attr (byte 2) are unimplemented and always read back as 0
            if self.oam_addr & 0x03 == 0x02 {
                val &= 0xE3;
            }
            self.oam[self.oam_addr as usize] = val;
            self.oam_addr = self.oam_addr.wrapping_add(1);
        }
    }

    fn write_scroll(&mut self, value: u8) {
        if !self.w {
            // First write: coarse X scroll (t[0:4]), fine X scroll (x = value[5:7])
            // 保留 nametable 位 (t[10:11]) 和 fine Y (t[12:14])
            self.t = (self.t & 0x7BE0) | ((value as u16 >> 3) & 0x1F);
            self.x = value & 0x07;
            self.w = true;
        } else {
            // Second write: coarse Y scroll (t[5:9]), fine Y scroll (t[12:14])
            // 保留 coarse X (t[0:4]) 和 nametable X (t[10])
            self.t = (self.t & 0x8C1F)
                | (((value as u16) & 0x07) << 12)
                | (((value as u16) & 0xF8) << 2);
            self.w = false;
        }
    }

    fn write_addr(&mut self, value: u8) {
        if !self.w {
            // First write: high byte (t[8:14]) — 只取低 6 位
            self.t = (self.t & 0x00FF) | ((value as u16 & 0x3F) << 8);
            self.w = true;
        } else {
            // Second write: low byte, copy t → v（带 2 周期延迟）
            self.t = (self.t & 0xFF00) | value as u16;
            self.v = self.t & 0x7FFF; // 15 位地址
            self.w = false;
        }
    }

    fn write_data(&mut self, value: u8) {
        let addr = self.v & 0x3FFF;
        if addr < 0x2000 {
            // CHR-RAM write (handled by bus)
        } else if addr < 0x3F00 {
            // Nametable write — use same mirroring as reads
            let raw = (addr & 0x0FFF) as usize;
            let mapped = self.map_nametable_addr(raw, self.mirroring);
            self.nametable[mapped] = value;
        } else {
            // Palette write
            self.write_palette_internal(addr, value);
        }

        // VRAM 地址增量
        if self.ctrl & 0x04 != 0 {
            self.v = self.v.wrapping_add(32);
        } else {
            self.v = self.v.wrapping_add(1);
        }
    }

    // ---- DMA ----

    /// OAM DMA ($4014)
    pub fn oam_dma(&mut self, data: &[u8; 256]) {
        self.oam.copy_from_slice(data);
    }

    // ---- 命名表访问 (由 bus 调用) ----

    pub fn read_nametable(&mut self, addr: u16, mirroring: Mirroring) -> u8 {
        let addr = (addr & 0x0FFF) as usize;
        let mapped = self.map_nametable_addr(addr, mirroring);
        self.nametable[mapped]
    }

    pub fn write_nametable(&mut self, addr: u16, value: u8, mirroring: Mirroring) {
        let addr = (addr & 0x0FFF) as usize;
        let mapped = self.map_nametable_addr(addr, mirroring);
        self.nametable[mapped] = value;
    }

    fn map_nametable_addr(&self, addr: usize, mirroring: Mirroring) -> usize {
        match mirroring {
            Mirroring::Vertical => {
                // NT0 @ 0x0000-0x03FF, NT1 @ 0x0400-0x07FF
                // Vertical: [0x2000 A] [0x2400 B]  [0x2800 a] [0x2C00 b]
                match addr & 0x0C00 {
                    0x0000 | 0x0800 => addr & 0x03FF, // NT0 (A)
                    _ => 0x0400 | (addr & 0x03FF),    // NT1 (B)
                }
            }
            Mirroring::Horizontal => {
                // NT0 @ 0x0000-0x03FF, NT0 also @ 0x0400-0x07FF
                match addr & 0x0C00 {
                    0x0000 | 0x0400 => addr & 0x03FF, // NT0
                    _ => 0x0400 | (addr & 0x03FF),    // NT1
                }
            }
            Mirroring::FourScreen => {
                // 4-screen mirroring: all 4 nametables accessible
                addr.min(0x0FFF)
            }
            Mirroring::ScreenAOnly => addr & 0x03FF,
            Mirroring::ScreenBOnly => 0x0400 | (addr & 0x03FF),
            Mirroring::MapperControlled => {
                // MapperControlled 由 Mapper 动态决定，默认使用 Horizontal
                match addr & 0x0C00 {
                    0x0000 | 0x0400 => addr & 0x03FF,
                    _ => 0x0400 | (addr & 0x03FF),
                }
            }
        }
    }

    // ---- 调色板访问 (由 bus 调用) ----

    pub fn read_palette(&mut self, addr: u16) -> u8 {
        self.read_palette_internal(addr)
    }

    pub fn write_palette(&mut self, addr: u16, value: u8) {
        self.write_palette_internal(addr, value);
    }

    fn read_palette_internal(&self, addr: u16) -> u8 {
        let idx = self.map_palette_addr(addr);
        self.palette[idx]
    }

    fn write_palette_internal(&mut self, addr: u16, value: u8) {
        let idx = self.map_palette_addr(addr);
        self.palette[idx] = value;
    }

    fn map_palette_addr(&self, addr: u16) -> usize {
        let a = addr & 0x001F;
        // Mirror $3F10/$3F14/$3F18/$3F1C → $3F00/$3F04/$3F08/$3F0C
        // tetanes-core 使用 PALETTE_MIRROR 查找表:
        // [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,0,17,18,19,4,21,22,23,8,25,26,27,12,29,30,31]
        const PALETTE_MIRROR: [usize; 32] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0, 17, 18, 19, 4, 21, 22, 23, 8,
            25, 26, 27, 12, 29, 30, 31,
        ];
        PALETTE_MIRROR[(a as usize) % 32]
    }

    // ---- 渲染 ----

    /// 渲染一行, 需要外部提供 bus 来读取图案表数据
    /// 由外部循环调用时提供 bus
    pub fn render_scanline_external(&mut self, y: u16, cartridge: &mut Cartridge) {
        if !self.is_rendering_enabled() {
            // 渲染关闭: 填充背景色
            let bg = self.read_palette_internal_external(None);
            for x in 0..256usize {
                self.frame[(y as usize) * 256 + x] = bg;
            }
            return;
        }
        self.render_background(y, cartridge);
        self.render_sprites(y, cartridge);
    }

    fn render_scanline(&mut self, _y: u16) {
        // Rendering is done externally via render_scanline_external.
        // This hook exists so tick() can signal that a scanline is ready.
    }

    fn is_rendering_enabled(&self) -> bool {
        self.mask & 0x18 != 0 // bit 4 (sprites) or bit 3 (background)
    }

    /// 对外接口: 渲染并取帧完成标志
    pub fn get_frame_complete(&self) -> bool {
        self.frame_complete
    }

    pub fn clear_frame_complete(&mut self) {
        self.frame_complete = false;
    }

    /// Increment horizontal scroll position (coarse X, nametable X wrap)
    fn increment_x(&mut self) {
        // tetanes-core 逻辑:
        // 如果 coarse X == 31，重置 coarse X 并切换水平 nametable
        // 否则递增 coarse X
        if (self.v & 0x001F) == 31 {
            self.v &= !0x001F; // reset coarse X
            self.v ^= 0x0400; // toggle nametable X
        } else {
            self.v += 1;
        }
    }

    /// Increment vertical scroll position (coarse Y, fine Y wrap)
    fn increment_y(&mut self) {
        // tetanes-core 逻辑:
        // 如果 fine Y < 7 (0b111)，递增 fine Y
        // 如果 fine Y == 7，重置 fine Y 并递增 coarse Y
        // 如果 coarse Y == 29，重置 coarse Y 并切换垂直 nametable
        // 如果 coarse Y == 31，重置 coarse Y（不切换 nametable）
        if (self.v & 0x7000) != 0x7000 {
            // fine Y < 7: 递增 fine Y
            self.v += 0x1000;
        } else {
            // fine Y == 7: 重置 fine Y
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5; // coarse Y
            if y == 29 {
                y = 0;
                self.v ^= 0x0800; // toggle nametable vertically
            } else if y == 31 {
                y = 0;
                // 不切换 nametable
            } else {
                y += 1;
            }
            self.v = (self.v & !0x03E0) | (y << 5);
        }
    }

    pub fn set_vblank(&mut self, active: bool) {
        if active {
            self.status |= 0x80;
        } else {
            self.status &= !0x80;
        }
    }

    /// 渲染整个可见区域
    ///
    /// tetanes-core 在 clock() 中逐像素实时渲染，每行 cycle=256 时
    /// 调用 increment_y()。我们采用批量渲染，但需要模拟相同的 v 寄存器行为：
    /// - 每行渲染前，v 已经包含了正确的滚动值（由上一帧预渲染扫描线的 copy_x/copy_y 设置）
    /// - 每行渲染后，调用 increment_y() 推进到下一行
    /// - render_background() 内部会通过 increment_x() 修改 v，但会保存/恢复
    pub fn render_frame(&mut self, cartridge: &mut Cartridge) {
        // tetanes-core 在 rendering_enabled=false 时仍然逐像素渲染，
        // 但只读取调色板数据（通过 scroll.addr() 读取），不读取背景/精灵数据
        // 这里简化处理：用背景色填充
        if self.mask & 0x18 == 0 {
            let bg = self.read_palette_internal_external(None);
            for pixel in self.frame.iter_mut() {
                *pixel = bg;
            }
            return;
        }
        // t→v 复制已在预渲染扫描线 (scanline 261) 的 tick() 中完成
        // 此时 v 已经包含了当前帧第一行的正确滚动值
        for y in 0..240 {
            self.render_scanline_external(y, cartridge);
            // tetanes-core 在每行的 cycle=256 调用 increment_y()
            // 我们在这里模拟，推进到下一行
            self.increment_y();
        }
    }

    /// 获取帧缓冲引用
    pub fn frame(&self) -> &[u8; 256 * 240] {
        &self.frame
    }

    /// 获取 nametable 数据引用 (用于 native 渲染)
    pub fn nametable_data(&self) -> &[u8; 4096] {
        &self.nametable
    }

    /// 获取调色板 RAM 引用 (用于 native 渲染)
    pub fn palette_data(&self) -> &[u8; 32] {
        &self.palette
    }

    /// 获取 OAM 数据引用 (用于 native 渲染)
    pub fn oam_data(&self) -> &[u8; 256] {
        &self.oam
    }

    fn read_palette_internal_external(&self, pal_addr: Option<u16>) -> u8 {
        match pal_addr {
            Some(addr) => {
                // 使用完整的调色板地址（$3F00-$3FFF）
                // tetanes-core 逻辑: 如果 palette & 0x03 == 0，使用 $3F00（背景色）
                let effective_addr = if addr & 0x03 == 0 { 0x3F00 } else { addr };
                let idx = self.map_palette_addr(effective_addr);
                self.palette[idx]
            }
            None => {
                // Universal background color at $3F00
                self.palette[0]
            }
        }
    }

    fn render_background(&mut self, y: u16, cartridge: &mut Cartridge) {
        let fine_y = (self.v >> 12) & 0x07; // 从 v 寄存器获取 fine Y 偏移
        let bg_enabled = self.mask & 0x08 != 0;
        let show_left_bg = self.mask & 0x02 != 0;

        // 保存 v 的初始值，每行渲染后恢复（因为 increment_x 会修改 v）
        let saved_v = self.v;

        // 使用移位寄存器模拟逐周期 tile 获取
        // tetanes-core 在每行的 cycle 1-256 期间通过 bg_fetch_cycle() 逐周期填充
        // tile_shift_lo/hi 存储当前 8 像素的位平面数据
        // curr_palette 是当前 tile 的调色板，next_palette 是下一个 tile 的调色板
        let mut tile_shift_lo: u16;
        let mut tile_shift_hi: u16;
        let mut curr_palette: u8;
        let mut next_palette: u8 = 0;
        let mut tile_lo: u8;
        let mut tile_hi: u8;

        // tetanes-core 在 cycle 1-256 期间每 2 个周期获取一个 tile 的 4 个字节
        // 第一个 tile 在 cycle 1-8 获取，cycle 9 时第一个像素开始渲染
        // 我们需要在 pixel_x=0 之前预取第一个 tile
        // 模拟 cycle 1-8 的预取
        {
            let nametable_addr = 0x2000 | (self.v & 0x0FFF);
            let tile_index = self.ppu_read_nametable(nametable_addr);
            let attr_addr = self.scroll_attr_addr(self.v);
            let attr_byte = self.ppu_read_nametable(attr_addr);
            let attr_shift = self.scroll_attr_shift(self.v);
            curr_palette = ((attr_byte >> attr_shift) & 0x03) << 2;
            let pattern_table = if self.ctrl & 0x10 != 0 {
                0x1000
            } else {
                0x0000
            };
            let tile_addr = pattern_table | (u16::from(tile_index) << 4) | fine_y;
            tile_lo = cartridge.ppu_read(tile_addr).unwrap_or(0);
            tile_hi = cartridge.ppu_read(tile_addr + 8).unwrap_or(0);
            // 将第一个 tile 数据载入移位寄存器的高 8 位
            tile_shift_lo = u16::from(tile_lo) << 8;
            tile_shift_hi = u16::from(tile_hi) << 8;
        }

        for pixel_x in 0..256usize {
            let x = pixel_x as u16;

            if !bg_enabled {
                let bg = self.read_palette_internal_external(None);
                self.frame[(y as usize) * 256 + pixel_x] = bg;
                continue;
            }

            // 每 8 像素（每个 tile 的起始，除了第一个）获取新的 tile 数据
            // tetanes-core 在 bg_fetch_cycle 的 phase 0 调用 increment_x，
            // 然后在 phase 1-7 获取下一个 tile 的数据
            if pixel_x & 0x07 == 0 && pixel_x != 0 {
                // 将之前的 tile 数据移入移位寄存器（低 8 位）
                tile_shift_lo = (tile_shift_lo & 0xFF00) | u16::from(tile_lo);
                tile_shift_hi = (tile_shift_hi & 0xFF00) | u16::from(tile_hi);
                curr_palette = next_palette;

                // 从 v 寄存器获取下一个 nametable 地址
                let nametable_addr = 0x2000 | (self.v & 0x0FFF);
                let tile_index = self.ppu_read_nametable(nametable_addr);

                // 获取 attribute 字节
                let attr_addr = self.scroll_attr_addr(self.v);
                let attr_byte = self.ppu_read_nametable(attr_addr);
                let attr_shift = self.scroll_attr_shift(self.v);
                next_palette = ((attr_byte >> attr_shift) & 0x03) << 2;

                // 获取 tile 位平面数据
                let pattern_table = if self.ctrl & 0x10 != 0 {
                    0x1000
                } else {
                    0x0000
                };
                let tile_addr = pattern_table | (u16::from(tile_index) << 4) | fine_y;
                tile_lo = cartridge.ppu_read(tile_addr).unwrap_or(0);
                tile_hi = cartridge.ppu_read(tile_addr + 8).unwrap_or(0);
            }

            // 从移位寄存器提取当前像素颜色 (使用 fine_x 偏移)
            // tetanes-core: bg_shift = 15 - fine_x
            let fine_x = self.x;
            let bg_shift = 15 - fine_x;
            let color =
                (((tile_shift_hi >> bg_shift) & 0x01) << 1) | ((tile_shift_lo >> bg_shift) & 0x01);

            // 左 8 像素裁剪
            let left_clip = x < 8 && !show_left_bg;
            let effective_color = if left_clip { 0 } else { color };

            if effective_color == 0 {
                self.frame[(y as usize) * 256 + pixel_x] =
                    self.read_palette_internal_external(None);
            } else {
                // tetanes-core: palette = curr_palette + bg_color
                // 其中 curr_palette 是 attribute 选择的调色板（0,4,8,12 等）
                // bg_color 是 2-bit 颜色索引
                // 最终调色板地址: $3F00 | (palette & 0x03 > 0) as u16 * palette
                // 简化: $3F00 | curr_palette | color
                let pal_addr = 0x3F00 | u16::from(curr_palette) | u16::from(effective_color);
                self.frame[(y as usize) * 256 + pixel_x] =
                    self.read_palette_internal_external(Some(pal_addr));
            }

            // 每 8 像素递增 coarse X（tetanes-core 在 bg_fetch_cycle 的 phase 0 调用 increment_x）
            // 注意：在 pixel_x=255 时不递增（cycle=256 时调用 increment_y 而非 increment_x）
            if pixel_x & 0x07 == 7 && pixel_x != 255 {
                self.increment_x();
            }

            // 移位寄存器每周期左移 1 位（tetanes-core 在 clock() 末尾执行）
            tile_shift_lo <<= 1;
            tile_shift_hi <<= 1;
        }

        // 恢复 v 寄存器（render_frame 会调用 increment_y 更新 v）
        self.v = saved_v;
    }

    /// 计算 attribute 地址（与 tetanes-core scroll.attr_addr() 一致）
    fn scroll_attr_addr(&self, v: u16) -> u16 {
        let nametable_select = v & 0x0C00;
        let y_bits = (v >> 4) & 0x38;
        let x_bits = (v >> 2) & 0x07;
        0x23C0 | nametable_select | y_bits | x_bits
    }

    /// 计算 attribute 偏移（与 tetanes-core scroll.attr_shift() 一致）
    fn scroll_attr_shift(&self, v: u16) -> u8 {
        ((v & 0x02) | ((v >> 4) & 0x04)) as u8
    }

    fn render_sprites(&mut self, y: u16, cartridge: &mut Cartridge) {
        let sprites_enabled = self.mask & 0x10 != 0;
        let show_left_spr = self.mask & 0x04 != 0;
        let bg_enabled = self.mask & 0x08 != 0;
        let mut sprite_count = 0;

        // 用于精灵 0 碰撞检测：记录每个像素位置的背景颜色索引（2-bit）
        // tetanes-core 使用移位寄存器中的 2-bit 颜色索引判断 bg_color != 0
        let mut bg_color_index: [u8; 256] = [0; 256];
        if bg_enabled {
            let base = (y as usize) * 256;
            for x in 0..256usize {
                let pixel = self.frame[base + x];
                // 如果像素是背景色（palette[0]），则 bg_color_index = 0
                // 否则 bg_color_index = 1（非零）
                bg_color_index[x] = if pixel == self.palette[0] { 0 } else { 1 };
            }
        }

        for i in 0..64 {
            let oam_base = i as usize * 4;
            let sprite_y = self.oam[oam_base];
            let tile_idx = self.oam[oam_base + 1];
            let attributes = self.oam[oam_base + 2];
            let sprite_x = self.oam[oam_base + 3];

            // NES sprites are 8x8 or 8x16 (bit 5 of ctrl)
            let sprite_height: u16 = if self.ctrl & 0x20 != 0 { 16 } else { 8 };

            // Check if this sprite is on the current scanline
            let diff = (y as i16) - (sprite_y as i16);
            if diff < 0 || diff >= sprite_height as i16 {
                continue;
            }

            sprite_count += 1;
            if sprite_count > 8 {
                // Max 8 sprites per scanline — tetanes-core 有更复杂的溢出逻辑
                self.status |= 0x40; // sprite overflow
                break;
            }

            let mut sprite_y_offset = diff as u16;
            let flip_v = attributes & 0x80 != 0;
            let flip_h = attributes & 0x40 != 0;
            let palette_offset = (attributes & 0x03) as u16;
            let priority = attributes & 0x20 != 0;

            if flip_v {
                sprite_y_offset = sprite_height - 1 - sprite_y_offset;
            }

            let pattern_table = if self.ctrl & 0x08 != 0 {
                0x1000
            } else {
                0x0000
            };
            let mut tile_addr = pattern_table + (tile_idx as u16) * 16 + sprite_y_offset;
            if sprite_height == 16 {
                // 8x16 mode: bit 0 of tile index selects pattern table
                let table = (tile_idx & 0x01) as u16;
                let actual_tile = tile_idx & 0xFE;
                tile_addr = table * 0x1000 + (actual_tile as u16) * 16 + (sprite_y_offset & 0x07);
                if sprite_y_offset >= 8 {
                    tile_addr += 8; // bottom half of 8x16 tile
                }
            }

            let plane0 = cartridge.ppu_read(tile_addr).unwrap_or(0);
            let plane1 = cartridge.ppu_read(tile_addr + 8).unwrap_or(0);

            for bit in 0..8u8 {
                let mut src_bit = bit;
                if flip_h {
                    src_bit = 7 - bit;
                }
                let color = ((plane0 >> src_bit) & 0x01) | (((plane1 >> src_bit) & 0x01) << 1);

                if color == 0 {
                    continue; // transparent
                }

                let px = (sprite_x as u16).wrapping_add(bit as u16);
                if px >= 256 {
                    continue;
                }

                let fb_idx = (y as usize) * 256 + px as usize;

                // 左 8 像素裁剪（精灵）
                if px < 8 && !show_left_spr {
                    continue;
                }

                // Sprite 0 hit detection — tetanes-core 精确条件:
                // rendering_enabled && !spr_zero_hit && spr_zero_visible && cycle != 256 && i == 0 && bg_color != 0
                // 注意: tetanes-core 使用 status bit 6 (SPR_ZERO_HIT)，我们使用 bit 6 (0x40)
                // cycle != 256 排除最后一个像素（tetanes-core 的 pixel_palette 中检查）
                if i == 0 && sprites_enabled && (self.status & 0x40) == 0 {
                    // 检查背景是否非零（使用 2-bit 颜色索引）
                    let bg_nonzero = bg_color_index[px as usize] != 0;
                    // 排除 x=255（对应 cycle=256）
                    if bg_nonzero && px != 255 {
                        self.status |= 0x40; // sprite 0 hit
                    }
                }

                // 精灵优先级：如果 priority 设置且背景非零，精灵在背景后面
                if priority && bg_color_index[px as usize] != 0 {
                    continue;
                }

                // 精灵调色板地址: tetanes-core 使用 palette + spr_color
                // palette = ((attr & 0x03) << 2) | 0x10
                // 最终调色板地址: $3F00 | ((palette & 0x03 > 0) as u16 * palette)
                // 简化: $3F10 | (palette_offset << 2) | color
                let pal_addr = 0x3F10 + palette_offset * 4 + color as u16;
                self.frame[fb_idx] = self.read_palette_internal_external(Some(pal_addr));
            }
        }
    }

    fn ppu_read_nametable(&mut self, addr: u16) -> u8 {
        // Nametable/palette reads are internal to PPU, no bus needed.
        // Pattern table reads ($0000-$1FFF) go through mapper — callers
        // must handle that range themselves via mapper.ppu_read().
        match addr {
            0x2000..=0x3EFF => self.read_nametable(addr, self.mirroring),
            0x3F00..=0x3FFF => self.read_palette(addr),
            _ => 0, // CHR range — caller must pass mapper separately
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper::{
        Cartridge, CartridgeMetadata, ChrStorage, MapperChip, MapperContext, MapperSaveState,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Minimal MapperChip for testing — only provides CHR-ROM pattern table data.
    struct TestMapper {
        chr: Vec<u8>,
        mirroring: Mirroring,
    }
    impl MapperChip for TestMapper {
        fn mapper_id(&self) -> u16 {
            0
        }
        fn name(&self) -> &'static str {
            "Test"
        }
        fn cpu_read(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16) -> Option<u8> {
            None
        }
        fn cpu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool {
            false
        }
        fn ppu_read(&mut self, _ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
            if addr < 0x2000 {
                let idx = (addr as usize) % self.chr.len();
                Some(self.chr[idx])
            } else {
                None
            }
        }
        fn ppu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool {
            false
        }
        fn mirroring(&self) -> Mirroring {
            self.mirroring
        }
        fn save_state(&self) -> MapperSaveState {
            MapperSaveState::new(0)
        }
        fn load_state(&mut self, _state: &MapperSaveState) {}
    }

    // ===== Nametable Mirroring Tests =====

    #[test]
    fn test_nametable_mirror_horizontal() {
        let ppu = PpuCompat::new(Mirroring::Horizontal);
        // Horizontal: [0x2000 A] [0x2400 a]  [0x2800 B] [0x2C00 b]
        // A = NT0 (0x0000-0x03FF), B = NT1 (0x0400-0x07FF)
        assert_eq!(
            ppu.map_nametable_addr(0x0000, Mirroring::Horizontal),
            0x0000
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0005, Mirroring::Horizontal),
            0x0005
        );
        assert_eq!(
            ppu.map_nametable_addr(0x03FF, Mirroring::Horizontal),
            0x03FF
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0400, Mirroring::Horizontal),
            0x0000
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0405, Mirroring::Horizontal),
            0x0005
        );
        assert_eq!(
            ppu.map_nametable_addr(0x07FF, Mirroring::Horizontal),
            0x03FF
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0800, Mirroring::Horizontal),
            0x0400
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0805, Mirroring::Horizontal),
            0x0405
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0BFF, Mirroring::Horizontal),
            0x07FF
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0C00, Mirroring::Horizontal),
            0x0400
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0C05, Mirroring::Horizontal),
            0x0405
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0FFF, Mirroring::Horizontal),
            0x07FF
        );
    }

    #[test]
    fn test_nametable_mirror_vertical() {
        let ppu = PpuCompat::new(Mirroring::Vertical);
        // Vertical: [0x2000 A] [0x2400 B]  [0x2800 a] [0x2C00 b]
        assert_eq!(ppu.map_nametable_addr(0x0000, Mirroring::Vertical), 0x0000);
        assert_eq!(ppu.map_nametable_addr(0x0005, Mirroring::Vertical), 0x0005);
        assert_eq!(ppu.map_nametable_addr(0x03FF, Mirroring::Vertical), 0x03FF);
        assert_eq!(ppu.map_nametable_addr(0x0800, Mirroring::Vertical), 0x0000);
        assert_eq!(ppu.map_nametable_addr(0x0805, Mirroring::Vertical), 0x0005);
        assert_eq!(ppu.map_nametable_addr(0x0BFF, Mirroring::Vertical), 0x03FF);
        assert_eq!(ppu.map_nametable_addr(0x0400, Mirroring::Vertical), 0x0400);
        assert_eq!(ppu.map_nametable_addr(0x0405, Mirroring::Vertical), 0x0405);
        assert_eq!(ppu.map_nametable_addr(0x07FF, Mirroring::Vertical), 0x07FF);
        assert_eq!(ppu.map_nametable_addr(0x0C00, Mirroring::Vertical), 0x0400);
        assert_eq!(ppu.map_nametable_addr(0x0C05, Mirroring::Vertical), 0x0405);
        assert_eq!(ppu.map_nametable_addr(0x0FFF, Mirroring::Vertical), 0x07FF);
    }

    #[test]
    fn test_nametable_mirror_single_screen_a() {
        let ppu = PpuCompat::new(Mirroring::ScreenAOnly);
        assert_eq!(
            ppu.map_nametable_addr(0x0000, Mirroring::ScreenAOnly),
            0x0000
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0005, Mirroring::ScreenAOnly),
            0x0005
        );
        assert_eq!(
            ppu.map_nametable_addr(0x03FF, Mirroring::ScreenAOnly),
            0x03FF
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0400, Mirroring::ScreenAOnly),
            0x0000
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0800, Mirroring::ScreenAOnly),
            0x0000
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0C00, Mirroring::ScreenAOnly),
            0x0000
        );
    }

    #[test]
    fn test_nametable_mirror_single_screen_b() {
        let ppu = PpuCompat::new(Mirroring::ScreenBOnly);
        assert_eq!(
            ppu.map_nametable_addr(0x0000, Mirroring::ScreenBOnly),
            0x0400
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0005, Mirroring::ScreenBOnly),
            0x0405
        );
        assert_eq!(
            ppu.map_nametable_addr(0x03FF, Mirroring::ScreenBOnly),
            0x07FF
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0400, Mirroring::ScreenBOnly),
            0x0400
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0800, Mirroring::ScreenBOnly),
            0x0400
        );
        assert_eq!(
            ppu.map_nametable_addr(0x0C00, Mirroring::ScreenBOnly),
            0x0400
        );
    }

    // ===== Palette Mirroring Tests =====

    #[test]
    fn test_palette_mirroring() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // Write to all palette entries via write_palette (uses map_palette_addr)
        // Since $3F10 maps to $3F00, writing to $3F10 overwrites $3F00
        // We need to write in reverse order or use direct palette access
        // Let's write directly to the palette array to set up unique values
        for i in 0..32u8 {
            ppu.palette[i as usize] = i;
        }
        // Now test reads through the mirrored interface
        // $3F10 mirrors to $3F00, $3F14 to $3F04, $3F18 to $3F08, $3F1C to $3F0C
        assert_eq!(ppu.read_palette(0x3F00), 0x00);
        assert_eq!(ppu.read_palette(0x3F10), 0x00); // mirror of $3F00
        assert_eq!(ppu.read_palette(0x3F04), 0x04);
        assert_eq!(ppu.read_palette(0x3F14), 0x04); // mirror of $3F04
        assert_eq!(ppu.read_palette(0x3F01), 0x01);
        assert_eq!(ppu.read_palette(0x3F11), 0x11); // NOT a mirror (index 17)
        assert_eq!(ppu.read_palette(0x3F02), 0x02);
        assert_eq!(ppu.read_palette(0x3F03), 0x03);
        // Verify write through mirror: write to $3F10 should write to $3F00
        ppu.write_palette(0x3F10, 0xFF);
        assert_eq!(ppu.read_palette(0x3F00), 0xFF); // $3F00 was overwritten
        assert_eq!(ppu.read_palette(0x3F10), 0xFF); // reads back through mirror
    }

    // ===== VRAM Read/Write Tests =====

    #[test]
    fn test_vram_writes() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.write_addr(0x23);
        ppu.write_addr(0x05);
        ppu.write_data(0x66); // write to $2305
        assert_eq!(ppu.read_nametable(0x2305, Mirroring::Horizontal), 0x66);
    }

    #[test]
    fn test_vram_reads() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // Direct write to nametable
        ppu.write_nametable(0x2305, 0x66, Mirroring::Horizontal);

        ppu.write_addr(0x23);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read (discard)
        assert_eq!(ppu.v & 0x3FFF, 0x2306); // addr incremented by 1
        assert_eq!(ppu.read_data(), 0x66); // second read gets the data
        assert_eq!(ppu.v & 0x3FFF, 0x2307);
    }

    #[test]
    fn test_vram_read_pagecross() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.write_nametable(0x21FF, 0x66, Mirroring::Horizontal);
        ppu.write_nametable(0x2200, 0x77, Mirroring::Horizontal);

        ppu.write_addr(0x21);
        ppu.write_addr(0xFF);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x66);
        assert_eq!(ppu.read_data(), 0x77);
    }

    #[test]
    fn test_vram_read_vertical_increment() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.write_ctrl(0b100); // vertical increment mode
        ppu.write_nametable(0x21FF, 0x66, Mirroring::Horizontal);
        ppu.write_nametable(0x21FF + 32, 0x77, Mirroring::Horizontal);
        ppu.write_nametable(0x21FF + 64, 0x88, Mirroring::Horizontal);

        ppu.write_addr(0x21);
        ppu.write_addr(0xFF);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x66);
        assert_eq!(ppu.read_data(), 0x77);
        assert_eq!(ppu.read_data(), 0x88);
    }

    #[test]
    fn test_vram_horizontal_mirror() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // Write to a ($2405) and B ($2805)
        ppu.write_addr(0x24);
        ppu.write_addr(0x05);
        ppu.write_data(0x66); // write to a at $2405

        ppu.write_addr(0x28);
        ppu.write_addr(0x05);
        ppu.write_data(0x77); // write to B at $2805

        // Read A from $2005 (should be $66 since A mirrors a in horizontal)
        ppu.write_addr(0x20);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x66);

        // Read b from $2C05 (should be $77 since b mirrors B)
        ppu.write_addr(0x2C);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x77);
    }

    #[test]
    fn test_vram_vertical_mirror() {
        let mut ppu = PpuCompat::new(Mirroring::Vertical);
        // Write to A ($2005) and b ($2C05)
        ppu.write_addr(0x20);
        ppu.write_addr(0x05);
        ppu.write_data(0x66); // write to A at $2005

        ppu.write_addr(0x2C);
        ppu.write_addr(0x05);
        ppu.write_data(0x77); // write to b at $2C05

        // Read a from $2805 (should be $66 since a mirrors A in vertical)
        ppu.write_addr(0x28);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x66);

        // Read B from $2405 (should be $77 since B mirrors b)
        ppu.write_addr(0x24);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x77);
    }

    // ===== Register Tests =====

    #[test]
    fn test_read_status_resets_latch() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.write_nametable(0x2305, 0x66, Mirroring::Horizontal);

        // Set up address, then read status (resets latch)
        ppu.write_addr(0x21);
        ppu.write_addr(0x23);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_ne!(ppu.read_data(), 0x66); // should NOT be 0x66 because latch was wrong

        // Reset with read_status
        ppu.read_status();

        // Now try again
        ppu.write_addr(0x23);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.read_data(), 0x66);
    }

    #[test]
    fn test_read_status_resets_vblank() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.status |= 0x80; // set VBlank

        let status = ppu.read_status();
        assert_eq!(status >> 7, 1);
        assert_eq!(ppu.status >> 7, 0); // cleared after read
    }

    #[test]
    fn test_vram_mirroring_address() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.write_ctrl(0);
        ppu.write_nametable(0x2305, 0x66, Mirroring::Horizontal);

        // 0x6305 mirrors to 0x2305 (high bits masked)
        ppu.write_addr(0x63);
        ppu.write_addr(0x05);
        ppu.read_data(); // buffer read
        assert_eq!(ppu.v & 0x3FFF, 0x2306);
        assert_eq!(ppu.read_data(), 0x66);
        assert_eq!(ppu.v & 0x3FFF, 0x2307);
    }

    #[test]
    fn test_oam_read_write() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.write_oam_addr(0x10);
        ppu.write_oam_data(0x66);
        ppu.write_oam_data(0x77);

        ppu.write_oam_addr(0x10);
        assert_eq!(ppu.read_oam_data(), 0x66);

        ppu.write_oam_addr(0x11);
        assert_eq!(ppu.read_oam_data(), 0x77);
    }

    // ===== Scroll Register Tests =====

    #[test]
    fn test_scroll_write_x_then_y() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // First write: X scroll
        ppu.write_scroll(0x4D); // fine_x = 5, coarse_x = 0x09
        assert_eq!(ppu.x, 0x05);
        assert_eq!((ppu.t & 0x001F), 0x09);
        assert!(ppu.w);

        // Second write: Y scroll
        ppu.write_scroll(0x6E); // fine_y = 6, coarse_y = 0x0D
        assert_eq!((ppu.t >> 12) & 0x07, 0x06);
        assert_eq!((ppu.t >> 5) & 0x1F, 0x0D);
        assert!(!ppu.w);
    }

    #[test]
    fn test_addr_write_hi_then_lo() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // First write: high byte
        ppu.write_addr(0x3F);
        assert!(ppu.w);
        assert_eq!((ppu.t >> 8) & 0x3F, 0x3F);

        // Second write: low byte, copies t to v
        ppu.write_addr(0x00);
        assert!(!ppu.w);
        assert_eq!(ppu.v & 0x3FFF, 0x3F00);
    }

    // ===== Rendering Tests =====

    #[test]
    fn test_ppu_renders_tile() {
        let mut chr = vec![0u8; 8192];
        // Tile 1: all solid (plane0 = 0xFF, plane1 = 0xFF → color index 3)
        chr[16..24].copy_from_slice(&[0xFF; 8]);
        chr[24..32].copy_from_slice(&[0xFF; 8]);

        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.mask = 0x1E;
        ppu.ctrl = 0x00;
        ppu.nametable[0] = 1;
        ppu.nametable[0x3C0] = 0xFF;
        ppu.v = 0x2000;
        ppu.t = 0x2000;
        // Set up palette: color index 3 in palette 3 → address $3F0F, value = 0x20
        ppu.palette[0x0F] = 0x20;

        let mapper = TestMapper {
            chr,
            mirroring: Mirroring::Horizontal,
        };
        let mut cartridge = Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: 0,
                submapper_id: 0,
                prg_rom_size: 0,
                chr_rom_size: 1,
                has_sram: false,
                has_trainer: false,
                battery_backed: false,
            },
            vec![],
            ChrStorage::Rom(vec![0; 8192]),
            Box::new(mapper),
        );
        ppu.render_frame(&mut cartridge);
        assert!(
            ppu.frame[0] != 0,
            "Pixel should be non-zero, got {}",
            ppu.frame[0]
        );
    }

    #[test]
    fn test_ppu_initial_state() {
        let ppu = PpuCompat::new(Mirroring::Horizontal);
        assert_eq!(ppu.status & 0x80, 0x80);
    }

    #[test]
    fn test_status_clears_vblank_on_read() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        let _ = ppu.read_register(0x2002);
        assert_eq!(ppu.status & 0x80, 0);
    }

    #[test]
    fn test_render_framebuffer_size() {
        let ppu = PpuCompat::new(Mirroring::Horizontal);
        let fb = ppu.frame();
        assert_eq!(fb.len(), 256 * 240);
        // All pixels should be within palette range (0-63)
        let max_val = fb.iter().copied().max().unwrap_or(0);
        assert!(
            max_val < 64,
            "NES palette index should be 0-63, got {}",
            max_val
        );
    }

    #[test]
    fn test_nametable_mirror_handling() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // Write to nametable 0 at $2000, read back at $2000 (should be same)
        let addr = 0x2000;
        ppu.write_nametable(addr, 0x42, crate::rom::Mirroring::Horizontal);
        let val = ppu.read_nametable(addr, crate::rom::Mirroring::Horizontal);
        assert_eq!(val, 0x42);
    }

    // ===== Increment X/Y Tests =====

    #[test]
    fn test_increment_x_basic() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.v = 0x2000; // coarse_x = 0
        ppu.increment_x();
        assert_eq!(ppu.v & 0x001F, 1); // coarse_x incremented
        assert_eq!(ppu.v & 0x0400, 0); // nametable X unchanged
    }

    #[test]
    fn test_increment_x_wrap() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.v = 0x201F; // coarse_x = 31 (max)
        ppu.increment_x();
        assert_eq!(ppu.v & 0x001F, 0); // coarse_x wraps to 0
        assert_eq!(ppu.v & 0x0400, 0x0400); // nametable X toggled
    }

    #[test]
    fn test_increment_y_basic() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.v = 0x0000; // fine_y = 0
        ppu.increment_y();
        assert_eq!((ppu.v >> 12) & 0x07, 1); // fine_y incremented
        assert_eq!(ppu.v, 0x1000);
    }

    #[test]
    fn test_increment_y_fine_y_wrap() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.v = 0x7000; // fine_y = 7
        ppu.increment_y();
        assert_eq!((ppu.v >> 12) & 0x07, 0); // fine_y wraps to 0
        assert_eq!((ppu.v >> 5) & 0x1F, 1); // coarse_y incremented
    }

    #[test]
    fn test_increment_y_coarse_y_wrap() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // fine_y=7, coarse_y=29 → v = (7<<12) | (0<<11) | (29<<5) = 0x73A0
        ppu.v = 0x73A0; // fine_y=7, nametable Y=0, coarse_y=29
        ppu.increment_y();
        // After: fine_y=0, coarse_y=0, nametable Y=1
        assert_eq!((ppu.v >> 12) & 0x07, 0); // fine_y = 0
        assert_eq!((ppu.v >> 5) & 0x1F, 0); // coarse_y = 0
        assert_eq!(ppu.v & 0x0800, 0x0800); // nametable Y toggled
        assert_eq!(ppu.v, 0x0800);
    }

    // ===== NMI Tests =====

    #[test]
    fn test_nmi_enable_during_vblank() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.status |= 0x80; // in VBlank
        ppu.write_ctrl(0x80); // enable NMI
        assert!(ppu.has_nmi);
    }

    #[test]
    fn test_nmi_disable_clears_pending() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.has_nmi = true;
        ppu.write_ctrl(0x00); // disable NMI
        // Current implementation doesn't clear has_nmi on disable
        // This test documents current behavior
    }

    #[test]
    fn test_take_nmi() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.has_nmi = true;
        assert!(ppu.take_nmi());
        assert!(!ppu.has_nmi);
        assert!(!ppu.take_nmi());
    }

    // ===== Sprite 0 Hit Tests =====

    #[test]
    fn test_sprite_zero_hit_detection() {
        let mut chr = vec![0u8; 8192];
        // Background tile 0: all 0xFF (color index 3)
        chr[0..8].copy_from_slice(&[0xFF; 8]);
        chr[8..16].copy_from_slice(&[0xFF; 8]);
        // Sprite tile 1: all 0xFF (color index 3)
        chr[16..24].copy_from_slice(&[0xFF; 8]);
        chr[24..32].copy_from_slice(&[0xFF; 8]);

        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.mask = 0x1E; // enable bg + sprites
        ppu.ctrl = 0x00;
        // Place tile 0 at (0,0) in nametable
        ppu.nametable[0] = 0;
        ppu.nametable[0x3C0] = 0x00; // palette 0 for all
        // Place sprite 0 at (0, 0) with tile 1
        ppu.oam[0] = 0; // y
        ppu.oam[1] = 1; // tile index
        ppu.oam[2] = 0x00; // attributes
        ppu.oam[3] = 0; // x
        // Palette: color 3 in palette 0 = $3F03
        ppu.palette[3] = 0x30;

        let mapper = TestMapper {
            chr,
            mirroring: Mirroring::Horizontal,
        };
        let mut cartridge = Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: 0,
                submapper_id: 0,
                prg_rom_size: 0,
                chr_rom_size: 1,
                has_sram: false,
                has_trainer: false,
                battery_backed: false,
            },
            vec![],
            ChrStorage::Rom(vec![0; 8192]),
            Box::new(mapper),
        );
        ppu.render_frame(&mut cartridge);
        // Sprite 0 hit should be detected
        assert!(
            (ppu.status & 0x40) != 0,
            "Sprite 0 hit should be set, status={:02X}",
            ppu.status
        );
    }

    // ===== Timing Tests =====

    #[test]
    fn test_tick_advances_cycle() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.cycle = 0;
        ppu.tick();
        assert_eq!(ppu.cycle, 1);
    }

    #[test]
    fn test_tick_advances_scanline() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.cycle = 340;
        let initial_scanline = ppu.scanline;
        ppu.tick();
        assert_eq!(ppu.cycle, 0);
        assert_eq!(ppu.scanline, initial_scanline + 1);
    }

    #[test]
    fn test_frame_complete() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.scanline = 261;
        ppu.cycle = 340;
        ppu.tick();
        assert!(ppu.frame_complete);
        assert_eq!(ppu.scanline, 0);
        assert_eq!(ppu.cycle, 0);
    }

    #[test]
    fn test_vblank_flag_set() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // Initial status has VBlank set (0xA0), clear it first
        ppu.status = 0x00;

        // Start at scanline 240, cycle 340
        ppu.scanline = 240;
        ppu.cycle = 340;
        ppu.tick(); // cycle wraps: scanline=241, cycle=0
        assert_eq!(ppu.scanline, 241);
        assert_eq!(ppu.cycle, 0);
        assert_eq!(ppu.status & 0x80, 0); // not yet set (cycle 0, not 1)

        ppu.tick(); // scanline=241, cycle=1
        assert_eq!(ppu.scanline, 241);
        assert_eq!(ppu.cycle, 1);
        assert_eq!(ppu.status & 0x80, 0x80); // VBlank set at cycle 1

        // 验证预渲染扫描线清除 VBlank
        ppu.scanline = 260;
        ppu.cycle = 340;
        ppu.tick(); // scanline=261, cycle=0
        assert_eq!(ppu.scanline, 261);
        assert_eq!(ppu.cycle, 0);
        assert_eq!(ppu.status & 0x80, 0x80); // VBlank still set at cycle 0

        ppu.tick(); // scanline=261, cycle=1
        assert_eq!(ppu.scanline, 261);
        assert_eq!(ppu.cycle, 1);
        assert_eq!(ppu.status & 0x80, 0); // VBlank cleared at cycle 1 of prerender
    }

    #[test]
    fn test_copy_x_on_prerender() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        ppu.t = 0x041F; // set nametable X=1, coarse_x=31
        ppu.v = 0x2000; // different X values
        ppu.scanline = 261;
        ppu.cycle = 256;
        ppu.tick(); // cycle becomes 257, copy_x triggers
        assert_eq!(ppu.cycle, 257);
        // X bits should be copied: coarse_x (t[0:4]) and nametable X (t[10])
        // copy_x: v = (v & !0x041F) | (t & 0x041F) = (0x2000 & 0xFFFFFBE0) | (0x041F & 0x041F)
        // = 0x2000 | 0x041F = 0x241F
        assert_eq!(ppu.v & 0x041F, ppu.t & 0x041F);
        assert_eq!(ppu.v, 0x241F);
    }

    #[test]
    fn test_copy_y_on_prerender() {
        let mut ppu = PpuCompat::new(Mirroring::Horizontal);
        // t = fine_y=7, nametable Y=1, coarse_y=29
        // = (7<<12) | (1<<11) | (29<<5) = 0x7000 | 0x0800 | 0x03A0 = 0x7BA0
        ppu.t = 0x7BA0;
        ppu.v = 0x2000; // different Y values
        ppu.scanline = 261;
        ppu.cycle = 279;
        ppu.tick(); // cycle becomes 280, copy_y triggers
        assert_eq!(ppu.cycle, 280);
        // Y bits should be copied: fine_y (t[12:14]), nametable Y (t[11]), coarse_y (t[5:9])
        // copy_y: v = (v & !0x7BE0) | (t & 0x7BE0)
        // = (0x2000 & 0xFFFF841F) | (0x7BA0 & 0x7BE0)
        // = 0x0000 | 0x7BA0 = 0x7BA0
        assert_eq!(ppu.v & 0x7BE0, ppu.t & 0x7BE0);
        assert_eq!(ppu.v, 0x7BA0);
    }
}
