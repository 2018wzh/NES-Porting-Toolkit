//! 三向帧缓冲比较
//!
//! 使用 tetanes-core 作为第三个参考实现，比较：
//! 1. NPTK 解释器 (NesSystem)
//! 2. NPTK 重编译 (RecompiledRuntime)
//! 3. tetanes-core (ControlDeck)
//!
//! 用于验证 NPTK 的渲染是否与知名模拟器 tetanes 一致。

use image::RgbImage;

use crate::compare::{FB_HEIGHT, FB_PIXELS, FB_WIDTH};

/// NES 标准调色板（64 色 RGB）
const NES_PALETTE: [(u8, u8, u8); 64] = nptk_wgpu::palette::NES_PALETTE;

/// tetanes-core 的帧缓冲格式
/// tetanes 使用 RGBA 格式，每像素 4 字节
const TETANES_FB_SIZE: usize = 256 * 240 * 4;

/// 三向比较结果
#[derive(Debug, Clone)]
pub struct TriComparisonResult {
    /// 总像素数
    pub total_pixels: usize,
    /// NPTK 解释器 vs 重编译 差异像素
    pub nptk_interp_vs_recomp_diff: usize,
    /// NPTK 解释器 vs tetanes 差异像素
    pub nptk_interp_vs_tetanes_diff: usize,
    /// NPTK 重编译 vs tetanes 差异像素
    pub nptk_recomp_vs_tetanes_diff: usize,
    /// 三者完全一致的像素
    pub all_identical: usize,
}

impl TriComparisonResult {
    /// 所有比较是否完全一致
    pub fn all_match(&self) -> bool {
        self.nptk_interp_vs_recomp_diff == 0
            && self.nptk_interp_vs_tetanes_diff == 0
            && self.nptk_recomp_vs_tetanes_diff == 0
    }

    /// 生成文本报告
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("=== 三向帧缓冲比较报告 ===\n\n");
        s.push_str(&format!("总像素: {}\n", self.total_pixels));
        s.push_str(&format!(
            "NPTK 解释器 vs 重编译: {} 差异像素 ({:.4}%)\n",
            self.nptk_interp_vs_recomp_diff,
            self.nptk_interp_vs_recomp_diff as f64 / self.total_pixels as f64 * 100.0
        ));
        s.push_str(&format!(
            "NPTK 解释器 vs tetanes: {} 差异像素 ({:.4}%)\n",
            self.nptk_interp_vs_tetanes_diff,
            self.nptk_interp_vs_tetanes_diff as f64 / self.total_pixels as f64 * 100.0
        ));
        s.push_str(&format!(
            "NPTK 重编译 vs tetanes: {} 差异像素 ({:.4}%)\n",
            self.nptk_recomp_vs_tetanes_diff,
            self.nptk_recomp_vs_tetanes_diff as f64 / self.total_pixels as f64 * 100.0
        ));
        s.push_str(&format!(
            "三者完全一致: {} 像素 ({:.4}%)\n\n",
            self.all_identical,
            self.all_identical as f64 / self.total_pixels as f64 * 100.0
        ));

        if self.all_match() {
            s.push_str("结果: 全部通过 ✓\n");
        } else {
            s.push_str("结果: 存在差异 ✗\n");
        }
        s
    }
}

/// 将 tetanes 的 RGBA 帧缓冲转换为 NES 索引色帧缓冲
///
/// tetanes 输出 RGBA 格式，我们需要找到最接近的 NES 调色板索引。
fn tetanes_rgba_to_nes_index(tetanes_fb: &[u8]) -> [u8; FB_PIXELS] {
    let mut nes_fb = [0u8; FB_PIXELS];

    for pixel_idx in 0..FB_PIXELS {
        let rgba_offset = pixel_idx * 4;
        let r = tetanes_fb[rgba_offset];
        let g = tetanes_fb[rgba_offset + 1];
        let b = tetanes_fb[rgba_offset + 2];

        // 找到最接近的 NES 调色板索引
        let mut best_idx = 0u8;
        let mut best_dist = u32::MAX;

        for (idx, &(pr, pg, pb)) in NES_PALETTE.iter().enumerate() {
            let dr = (r as i32 - pr as i32).abs() as u32;
            let dg = (g as i32 - pg as i32).abs() as u32;
            let db = (b as i32 - pb as i32).abs() as u32;
            let dist = dr + dg + db;

            if dist < best_dist {
                best_dist = dist;
                best_idx = idx as u8;
            }
        }

        nes_fb[pixel_idx] = best_idx;
    }

    nes_fb
}

/// 将 NES 索引色帧缓冲转换为 RGBA 图像数据
fn index_to_rgba(fb: &[u8; FB_PIXELS]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(FB_PIXELS * 4);
    for &idx in fb.iter() {
        let (r, g, b) = NES_PALETTE[idx as usize % 64];
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(255); // Alpha
    }
    rgba
}

/// 创建三向比较图像（NPTK 解释器 | NPTK 重编译 | tetanes | 差异高亮）
pub fn create_tri_comparison_image(
    nptk_interp_fb: &[u8; FB_PIXELS],
    nptk_recomp_fb: &[u8; FB_PIXELS],
    tetanes_fb: &[u8; FB_PIXELS],
) -> RgbImage {
    let img_width = FB_WIDTH * 4;
    let img_height = FB_HEIGHT;
    let mut img = RgbImage::new(img_width, img_height);

    for y in 0..FB_HEIGHT {
        for x in 0..FB_WIDTH {
            let idx = (y as usize) * FB_WIDTH as usize + (x as usize);

            // 栏 1: NPTK 解释器
            let (r, g, b) = NES_PALETTE[nptk_interp_fb[idx] as usize % 64];
            img.put_pixel(x, y, image::Rgb([r, g, b]));

            // 栏 2: NPTK 重编译
            let (r, g, b) = NES_PALETTE[nptk_recomp_fb[idx] as usize % 64];
            img.put_pixel(FB_WIDTH + x, y, image::Rgb([r, g, b]));

            // 栏 3: tetanes
            let (r, g, b) = NES_PALETTE[tetanes_fb[idx] as usize % 64];
            img.put_pixel(FB_WIDTH * 2 + x, y, image::Rgb([r, g, b]));

            // 栏 4: 差异高亮
            let all_same = nptk_interp_fb[idx] == nptk_recomp_fb[idx]
                && nptk_interp_fb[idx] == tetanes_fb[idx];

            if all_same {
                let (r, g, b) = NES_PALETTE[nptk_interp_fb[idx] as usize % 64];
                img.put_pixel(FB_WIDTH * 3 + x, y, image::Rgb([r, g, b]));
            } else {
                // 红色 = 三者不一致
                // 蓝色 = NPTK 内部一致但与 tetanes 不同
                // 黄色 = NPTK 内部就不一致
                if nptk_interp_fb[idx] == nptk_recomp_fb[idx] {
                    img.put_pixel(FB_WIDTH * 3 + x, y, image::Rgb([0, 0, 255])); // 蓝色
                } else {
                    img.put_pixel(FB_WIDTH * 3 + x, y, image::Rgb([255, 255, 0])); // 黄色
                }
            }
        }
    }

    img
}

/// 保存 RGB 图像为 PNG
pub fn save_rgb_png(
    img: &RgbImage,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// 比较三个帧缓冲
pub fn compare_three_framebuffers(
    nptk_interp: &[u8; FB_PIXELS],
    nptk_recomp: &[u8; FB_PIXELS],
    tetanes: &[u8; FB_PIXELS],
) -> TriComparisonResult {
    let mut interp_vs_recomp = 0usize;
    let mut interp_vs_tetanes = 0usize;
    let mut recomp_vs_tetanes = 0usize;
    let mut all_identical = 0usize;

    for i in 0..FB_PIXELS {
        let i_r = nptk_interp[i] == nptk_recomp[i];
        let i_t = nptk_interp[i] == tetanes[i];
        let r_t = nptk_recomp[i] == tetanes[i];

        if !i_r {
            interp_vs_recomp += 1;
        }
        if !i_t {
            interp_vs_tetanes += 1;
        }
        if !r_t {
            recomp_vs_tetanes += 1;
        }
        if i_r && i_t {
            all_identical += 1;
        }
    }

    TriComparisonResult {
        total_pixels: FB_PIXELS,
        nptk_interp_vs_recomp_diff: interp_vs_recomp,
        nptk_interp_vs_tetanes_diff: interp_vs_tetanes,
        nptk_recomp_vs_tetanes_diff: recomp_vs_tetanes,
        all_identical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_three_frames() {
        let fb = [42u8; FB_PIXELS];
        let result = compare_three_framebuffers(&fb, &fb, &fb);
        assert!(result.all_match());
        assert_eq!(result.all_identical, FB_PIXELS);
    }

    #[test]
    fn test_all_different_frames() {
        let fb1 = [0u8; FB_PIXELS];
        let fb2 = [1u8; FB_PIXELS];
        let fb3 = [2u8; FB_PIXELS];
        let result = compare_three_framebuffers(&fb1, &fb2, &fb3);
        assert!(!result.all_match());
        assert_eq!(result.all_identical, 0);
        assert_eq!(result.nptk_interp_vs_recomp_diff, FB_PIXELS);
        assert_eq!(result.nptk_interp_vs_tetanes_diff, FB_PIXELS);
        assert_eq!(result.nptk_recomp_vs_tetanes_diff, FB_PIXELS);
    }
}
