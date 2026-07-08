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
pub struct PpuCompat {
    // 寄存器
    pub ctrl: u8,   // $2000
    pub mask: u8,   // $2001
    pub status: u8, // $2002
    pub mirroring: Mirroring,
    oam_addr: u8,    // $2003
    data_buffer: u8, // $2007 read buffer

    // 内部地址 (NESDev: v, t, x, w)
    v: u16,  // current VRAM address (15 bit)
    t: u16,  // temporary VRAM address (15 bit)
    x: u8,   // fine X scroll (3 bit)
    w: bool, // write toggle
    pub has_nmi: bool,

    // 存储
    nametable: [u8; 4096], // 4KB 显存 (4 nametables for FourScreen support)
    oam: [u8; 256],        // 精灵属性内存
    palette: [u8; 32],     // 调色板

    // 帧缓冲 256×240
    frame: [u8; 256 * 240],

    // 渲染状态
    pub scanline: u16, // 当前扫描行 (0-261)
    pub cycle: u16,    // 当前周期 (0-340)
    frame_complete: bool,
    nmi_previous: bool,
    nmi_delay: u8,

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
            v: 0,
            t: 0,
            x: 0,
            w: false,
            has_nmi: false,
            nametable: [0; 4096],
            oam: [0; 256],
            palette: [
                0x0F, 0x00, 0x10, 0x20, 0x0F, 0x06, 0x16, 0x26, 0x0F, 0x08, 0x18, 0x28, 0x0F, 0x0A,
                0x1A, 0x2A, 0x0F, 0x0C, 0x1C, 0x2C, 0x0F, 0x0E, 0x1E, 0x2E, 0x0F, 0x01, 0x11, 0x21,
                0x0F, 0x05, 0x15, 0x25,
            ],
            frame: [0; 256 * 240],
            scanline: 241,
            cycle: 0,
            frame_complete: false,
            nmi_previous: false,
            nmi_delay: 0,
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
        if self.scanline >= 240 && self.scanline < 261 {
            // VBlank
            if self.scanline == 241 && self.cycle == 1 {
                // Set VBlank flag
                self.status |= 0x80;
                self.status &= !0x40; // clear sprite 0 hit for new frame
                if self.ctrl & 0x80 != 0 {
                    self.has_nmi = true;
                }
            }
        }

        if self.scanline < 240 {
            // Visible scanline — render at cycle 256 (after fetching tiles)
            if self.cycle == 256 {
                self.render_scanline(self.scanline);
            }
        }

        // 扫描线/周期更新
        self.cycle += 1;
        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame_complete = true;
                self.odd_frame = !self.odd_frame;
                // 在下一帧开始时清除 VBlank
                self.status &= !0x80;
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
            0x2000 => 0, // $2000 只写
            0x2001 => 0, // $2001 只写
            0x2002 => self.read_status(),
            0x2003 => 0, // $2003 只写
            0x2004 => self.read_oam_data(),
            0x2005 => 0, // $2005 只写
            0x2006 => 0, // $2006 只写
            0x2007 => self.read_data(),
            _ => 0,
        }
    }

    pub fn write_register(&mut self, addr: u16, value: u8) {
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
        let result = self.status;
        // Clear VBlank flag and write toggle
        self.status &= !0x80;
        self.w = false;
        result
    }

    fn read_oam_data(&mut self) -> u8 {
        self.oam[self.oam_addr as usize]
    }

    fn read_data(&mut self) -> u8 {
        let addr = self.v & 0x3FFF;
        let result = self.data_buffer;

        // 更新 buffer (从有效地址读取)
        if addr < 0x3F00 {
            // 图案表或 nametable 区域 — 用 buffer 实现延迟读取
            self.data_buffer = self.ppu_read_internal(addr);
        } else {
            self.data_buffer = self.ppu_read_internal(addr & !0x0F00);
        }

        // 调色板读取 — 直接返回数据 (非缓冲)
        if addr >= 0x3F00 {
            return self.ppu_read_internal(addr);
        }

        // VRAM 地址增量
        if self.ctrl & 0x04 != 0 {
            self.v = self.v.wrapping_add(32);
        } else {
            self.v = self.v.wrapping_add(1);
        }

        result
    }

    fn write_ctrl(&mut self, value: u8) {
        let old_nmi = self.ctrl & 0x80;
        self.ctrl = value;
        // t[10:12] = value[0:2] — nametable select
        self.t = (self.t & 0xF3FF) | ((value as u16 & 0x03) << 10);
        // NMI enabled changed
        if old_nmi == 0 && (value & 0x80) != 0 && (self.status & 0x80) != 0 {
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
        let addr = self.oam_addr as usize;
        self.oam[addr] = value;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    fn write_scroll(&mut self, value: u8) {
        if !self.w {
            // First write: coarse X scroll (t[0:4]), fine X scroll (x = value[5:7])
            self.t = (self.t & 0xFFE0) | ((value as u16 >> 3) & 0x1F);
            self.x = value & 0x07;
            self.w = true;
        } else {
            // Second write: coarse Y scroll (t[5:9]), fine Y scroll (t[12:14])
            self.t = (self.t & 0xFC1F)
                | (((value as u16) & 0x07) << 12)
                | (((value as u16) & 0xF8) << 2);
            self.w = false;
        }
    }

    fn write_addr(&mut self, value: u8) {
        if !self.w {
            // First write: high byte (t[8:14])
            self.t = (self.t & 0x80FF) | ((value as u16 & 0x3F) << 8);
            self.w = true;
        } else {
            // Second write: low byte, copy t → v
            self.t = (self.t & 0xFF00) | value as u16;
            self.v = self.t;
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
                // NT0 @ 0x0000-0x03FF, NT1 @ 0x0400-0x07FF, 水平镜像重复
                match addr & 0x0C00 {
                    0x0000 | 0x0C00 => addr & 0x03FF, // NT0
                    _ => 0x0400 | (addr & 0x03FF),    // NT1
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
        let idx = if a & 0x03 == 0 { a & 0x0F } else { a };
        idx as usize % 32
    }

    // ---- 内部 PPU 读取 (从总线获取图案表数据, 从内部获取名称表/调色板) ----

    fn ppu_read_internal(&self, _addr: u16) -> u8 {
        0
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

    /// Increment vertical scroll position (coarse Y, fine Y wrap)
    fn increment_y(&mut self) {
        if (self.v & 0x7000) != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5;
            if y == 29 {
                y = 0;
                self.v ^= 0x0800; // toggle nametable vertically
            } else if y == 31 {
                y = 0;
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
    pub fn render_frame(&mut self, cartridge: &mut Cartridge) {
        if self.mask & 0x18 == 0 {
            // 渲染关闭: 用背景色填充整个帧缓冲
            let bg = self.read_palette_internal_external(None);
            for pixel in self.frame.iter_mut() {
                *pixel = bg;
            }
            return;
        }
        // Copy t→v for rendering (pre-render scanline behavior)
        self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
        for y in 0..240 {
            self.render_scanline_external(y, cartridge);
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

    fn read_palette_internal_external(&self, pal_idx: Option<u8>) -> u8 {
        // Universal background color at $3F00
        let idx = match pal_idx {
            Some(i) => (i & 0x1F) as usize,
            None => 0,
        };
        self.palette[idx]
    }

    fn render_background(&mut self, y: u16, cartridge: &mut Cartridge) {
        let fine_y = (y.wrapping_add(self.v >> 12)) & 0x07;
        let tile_row = ((self.v >> 8) & 0x0F) as u16; // coarse Y from v
        let nametable_base = if self.ctrl & 0x01 != 0 {
            0x2800
        } else {
            0x2000
        };
        let bg_enabled = self.mask & 0x08 != 0;

        for pixel_x in 0..256usize {
            let x = pixel_x as u16;
            let tile_col = (x >> 3) as u16 & 0x1F; // which tile horizontally (0-31)

            if !bg_enabled {
                let bg = self.read_palette_internal_external(None);
                self.frame[(y as usize) * 256 + pixel_x] = bg;
                continue;
            }

            // Determine which nametable based on v's bit 10 (NT select from ctrl)
            let nt_offset = tile_row * 32 + tile_col;

            // Get tile index from nametable
            let tile_nt_addr = nametable_base + nt_offset;
            let tile_idx = self.ppu_read_nametable(tile_nt_addr);

            // Attribute byte for this tile (every 4x4 tile group)
            let attr_offset = 0x03C0 + (tile_row / 4) * 8 + (tile_col / 4);
            let attr_addr = nametable_base + attr_offset;
            let attr_byte = self.ppu_read_nametable(attr_addr);
            let pal_shift = ((tile_col % 4) / 2) * 2 + ((tile_row % 4) / 2) * 4;
            let pal_attr = (attr_byte >> pal_shift) & 0x03;

            // Which pattern table (bit 4 of ctrl)
            let pattern_table = if self.ctrl & 0x10 != 0 {
                0x1000
            } else {
                0x0000
            };
            let tile_addr = pattern_table + (tile_idx as u16) * 16 + fine_y;
            let plane0 = cartridge.ppu_read(tile_addr).unwrap_or(0);
            let plane1 = cartridge.ppu_read(tile_addr + 8).unwrap_or(0);

            let fine_x = (x & 0x07) as u8;
            let bit = 7 - fine_x;
            let color = ((plane0 >> bit) & 0x01) | (((plane1 >> bit) & 0x01) << 1);

            if color == 0 {
                self.frame[(y as usize) * 256 + pixel_x] =
                    self.read_palette_internal_external(None);
            } else {
                let pal_addr = 0x3F00 + (pal_attr as u16) * 4 + color as u16;
                self.frame[(y as usize) * 256 + pixel_x] =
                    self.read_palette_internal_external(Some(pal_addr as u8));
            }
        }
    }

    fn render_sprites(&mut self, y: u16, cartridge: &mut Cartridge) {
        let _sprites_enabled = self.mask & 0x10 != 0;
        let mut sprite_count = 0;

        for i in 0..64 {
            let oam_base = i as usize * 4;
            let sprite_y = self.oam[oam_base];
            let tile_idx = self.oam[oam_base + 1];
            let attributes = self.oam[oam_base + 2];
            let sprite_x = self.oam[oam_base + 3];

            // NES sprites are 8x8 or 8x16, here 8x8 (bit 5 of ctrl)
            let sprite_height: u16 = if self.ctrl & 0x20 != 0 { 16 } else { 8 };

            // Check if this sprite is on the current scanline
            let diff = (y as i16) - (sprite_y as i16);
            if diff < 0 || diff >= sprite_height as i16 {
                continue;
            }

            sprite_count += 1;
            if sprite_count > 8 {
                // Max 8 sprites per scanline
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
                if px >= 256 || px >= 256 {
                    continue;
                }

                let fb_idx = (y as usize) * 256 + px as usize;

                // Sprite 0 hit detection
                if i == 0 && color != 0 && self.mask & 0x08 != 0 {
                    let bg_color = self.read_palette_internal_external(None);
                    if self.frame[fb_idx] != bg_color {
                        self.status |= 0x20; // sprite 0 hit (bit 5)
                    }
                }

                if priority && self.frame[fb_idx] != 0 {
                    continue; // behind background
                }

                let pal_addr = 0x3F10 + palette_offset * 4 + color as u16;
                self.frame[fb_idx] = self.read_palette_internal_external(Some(pal_addr as u8));
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
        Cartridge, CartridgeMetadata, ChrStorage, MapperChip, MapperContext,
        MapperSaveState,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Minimal MapperChip for testing — only provides CHR-ROM pattern table data.
    struct TestMapper {
        chr: Vec<u8>,
        mirroring: Mirroring,
    }
    impl MapperChip for TestMapper {
        fn mapper_id(&self) -> u16 { 0 }
        fn name(&self) -> &'static str { "Test" }
        fn cpu_read(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16) -> Option<u8> { None }
        fn cpu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool { false }
        fn ppu_read(&mut self, _ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
            if addr < 0x2000 {
                let idx = (addr as usize) % self.chr.len();
                Some(self.chr[idx])
            } else {
                None
            }
        }
        fn ppu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool { false }
        fn mirroring(&self) -> Mirroring { self.mirroring }
        fn save_state(&self) -> MapperSaveState { MapperSaveState::new(0) }
        fn load_state(&mut self, _state: &MapperSaveState) {}
    }

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

        let mapper = TestMapper { chr, mirroring: Mirroring::Horizontal };
        let mut cartridge = Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: 0, submapper_id: 0,
                prg_rom_size: 0, chr_rom_size: 1,
                has_sram: false, has_trainer: false, battery_backed: false,
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
}
