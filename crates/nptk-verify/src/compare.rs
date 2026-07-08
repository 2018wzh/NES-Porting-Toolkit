//! 帧缓冲比较逻辑
//!
//! 提供逐像素比较两个 256×240 NES 帧缓冲（索引色，0-63）的功能，
//! 并生成差异高亮图像。

use image::RgbImage;

/// NES 标准调色板（64 色 RGB），从 nptk-wgpu 复用
const NES_PALETTE: [(u8, u8, u8); 64] = nptk_wgpu::palette::NES_PALETTE;

/// 帧缓冲宽度
pub const FB_WIDTH: u32 = 256;
/// 帧缓冲高度
pub const FB_HEIGHT: u32 = 240;
/// 帧缓冲总像素数
pub const FB_PIXELS: usize = (FB_WIDTH * FB_HEIGHT) as usize;

/// 单帧逐像素比较结果
#[derive(Debug, Clone)]
pub struct FramebufferDiff {
    /// 总像素数
    pub total_pixels: usize,
    /// 差异像素数
    pub mismatched_pixels: usize,
    /// 最大单像素差异（索引值差）
    pub max_diff: u8,
    /// 平均差异（仅差异像素）
    pub mean_diff: f64,
    /// 差异像素列表：(x, y, expected_index, actual_index)
    /// 最多记录前 1024 个差异位置，避免内存爆炸
    pub mismatches: Vec<(u16, u16, u8, u8)>,
}

impl FramebufferDiff {
    /// 两帧完全一致
    pub fn is_identical(&self) -> bool {
        self.mismatched_pixels == 0
    }

    /// 差异率（0.0 ~ 1.0）
    pub fn ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            return 0.0;
        }
        self.mismatched_pixels as f64 / self.total_pixels as f64
    }
}

/// 比较两个帧缓冲，返回逐像素差异
///
/// `expected` — 参考帧（通常是软件渲染器输出）
/// `actual` — 待验证帧（通常是原生渲染器输出）
///
/// 两个帧缓冲都是 256×240 的 NES 索引色（0-63）。
pub fn compare_framebuffers(
    expected: &[u8; FB_PIXELS],
    actual: &[u8; FB_PIXELS],
) -> FramebufferDiff {
    let total_pixels = FB_PIXELS;
    let mut mismatched_pixels = 0usize;
    let mut max_diff: u8 = 0;
    let mut diff_sum: u64 = 0;
    let mut mismatches: Vec<(u16, u16, u8, u8)> = Vec::new();

    // 最多记录 1024 个差异位置
    const MAX_RECORDED: usize = 1024;

    for y in 0..FB_HEIGHT as u16 {
        for x in 0..FB_WIDTH as u16 {
            let idx = (y as usize) * FB_WIDTH as usize + (x as usize);
            let e = expected[idx];
            let a = actual[idx];

            if e != a {
                let diff = if e > a { e - a } else { a - e };
                if diff > max_diff {
                    max_diff = diff;
                }
                diff_sum += diff as u64;
                mismatched_pixels += 1;

                if mismatches.len() < MAX_RECORDED {
                    mismatches.push((x, y, e, a));
                }
            }
        }
    }

    let mean_diff = if mismatched_pixels > 0 {
        diff_sum as f64 / mismatched_pixels as f64
    } else {
        0.0
    };

    FramebufferDiff {
        total_pixels,
        mismatched_pixels,
        max_diff,
        mean_diff,
        mismatches,
    }
}

/// 差异高亮图布局模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffImageLayout {
    /// 仅差异高亮图（差异像素红色，匹配像素原色）
    DiffOnly,
    /// 并排三栏：参考帧 | 待验证帧 | 差异高亮
    SideBySide,
    /// 上下三行：参考帧 / 待验证帧 / 差异高亮
    Stacked,
}

/// 将 NES 索引色帧缓冲转换为 RGB 图像数据
pub fn index_to_rgb(fb: &[u8; FB_PIXELS]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(FB_PIXELS * 3);
    for &idx in fb.iter() {
        let (r, g, b) = NES_PALETTE[idx as usize % 64];
        rgb.push(r);
        rgb.push(g);
        rgb.push(b);
    }
    rgb
}

/// 生成差异高亮图像（PNG 编码的字节）
///
/// * `diff` — 比较结果
/// * `expected` — 参考帧（索引色）
/// * `actual` — 待验证帧（索引色）
/// * `layout` — 布局模式
///
/// 返回 PNG 编码的字节向量。
pub fn diff_to_image(
    _diff: &FramebufferDiff,
    expected: &[u8; FB_PIXELS],
    actual: &[u8; FB_PIXELS],
    layout: DiffImageLayout,
) -> Vec<u8> {
    let (img_width, img_height) = match layout {
        DiffImageLayout::DiffOnly => (FB_WIDTH, FB_HEIGHT),
        DiffImageLayout::SideBySide => (FB_WIDTH * 3, FB_HEIGHT),
        DiffImageLayout::Stacked => (FB_WIDTH, FB_HEIGHT * 3),
    };

    let mut img = RgbImage::new(img_width, img_height);

    // 辅助函数：将索引色像素写入指定位置
    let set_pixel = |img: &mut RgbImage, x: u32, y: u32, idx: u8| {
        let (r, g, b) = NES_PALETTE[idx as usize % 64];
        img.put_pixel(x, y, image::Rgb([r, g, b]));
    };

    // 辅助函数：写入差异高亮像素
    let set_diff_pixel = |img: &mut RgbImage, x: u32, y: u32, is_diff: bool, orig_idx: u8| {
        if is_diff {
            // 差异像素：红色高亮
            img.put_pixel(x, y, image::Rgb([255, 0, 0]));
        } else {
            // 匹配像素：原色显示
            let (r, g, b) = NES_PALETTE[orig_idx as usize % 64];
            img.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    };

    match layout {
        DiffImageLayout::DiffOnly => {
            for y in 0..FB_HEIGHT {
                for x in 0..FB_WIDTH {
                    let idx = (y * FB_WIDTH + x) as usize;
                    let is_diff = expected[idx] != actual[idx];
                    set_diff_pixel(&mut img, x, y, is_diff, expected[idx]);
                }
            }
        }
        DiffImageLayout::SideBySide => {
            for y in 0..FB_HEIGHT {
                for x in 0..FB_WIDTH {
                    let idx = (y * FB_WIDTH + x) as usize;
                    // 左栏：参考帧
                    set_pixel(&mut img, x, y, expected[idx]);
                    // 中栏：待验证帧
                    set_pixel(&mut img, FB_WIDTH + x, y, actual[idx]);
                    // 右栏：差异高亮
                    let is_diff = expected[idx] != actual[idx];
                    set_diff_pixel(&mut img, FB_WIDTH * 2 + x, y, is_diff, expected[idx]);
                }
            }
        }
        DiffImageLayout::Stacked => {
            for y in 0..FB_HEIGHT {
                for x in 0..FB_WIDTH {
                    let idx = (y * FB_WIDTH + x) as usize;
                    // 上行：参考帧
                    set_pixel(&mut img, x, y, expected[idx]);
                    // 中行：待验证帧
                    set_pixel(&mut img, x, FB_HEIGHT + y, actual[idx]);
                    // 下行：差异高亮
                    let is_diff = expected[idx] != actual[idx];
                    set_diff_pixel(&mut img, x, FB_HEIGHT * 2 + y, is_diff, expected[idx]);
                }
            }
        }
    }

    // 编码为 PNG
    let mut png_bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .expect("Failed to encode PNG");
    png_bytes
}

// ── 计算帧哈希（用于快速比较） ──

/// 计算帧缓冲的简单哈希值
///
/// 用于快速判断两帧是否一致，避免逐像素比较。
pub fn frame_hash(fb: &[u8; FB_PIXELS]) -> u32 {
    fb.iter()
        .enumerate()
        .map(|(i, &p)| (p as u32).wrapping_mul((i as u32 % 251) + 1))
        .fold(0, |a, b| a ^ b)
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_frames() {
        let fb = [42u8; FB_PIXELS];
        let diff = compare_framebuffers(&fb, &fb);
        assert!(diff.is_identical());
        assert_eq!(diff.mismatched_pixels, 0);
        assert_eq!(diff.max_diff, 0);
        assert_eq!(diff.ratio(), 0.0);
    }

    #[test]
    fn test_completely_different_frames() {
        let expected = [0u8; FB_PIXELS];
        let actual = [63u8; FB_PIXELS];
        let diff = compare_framebuffers(&expected, &actual);
        assert!(!diff.is_identical());
        assert_eq!(diff.mismatched_pixels, FB_PIXELS);
        assert_eq!(diff.max_diff, 63);
        assert!(diff.ratio() > 0.99);
    }

    #[test]
    fn test_single_pixel_difference() {
        let mut expected = [0u8; FB_PIXELS];
        let mut actual = [0u8; FB_PIXELS];
        // 改变 (10, 20) 处的像素
        actual[20 * FB_WIDTH as usize + 10] = 1;
        expected[20 * FB_WIDTH as usize + 10] = 2;

        let diff = compare_framebuffers(&expected, &actual);
        assert_eq!(diff.mismatched_pixels, 1);
        assert_eq!(diff.max_diff, 1);
        assert_eq!(diff.mismatches.len(), 1);
        assert_eq!(diff.mismatches[0], (10, 20, 2, 1));
    }

    #[test]
    fn test_frame_hash_consistency() {
        let fb = [7u8; FB_PIXELS];
        let h1 = frame_hash(&fb);
        let h2 = frame_hash(&fb);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_frame_hash_different() {
        let fb1 = [0u8; FB_PIXELS];
        let fb2 = [1u8; FB_PIXELS];
        assert_ne!(frame_hash(&fb1), frame_hash(&fb2));
    }

    #[test]
    fn test_diff_to_image_diff_only() {
        let expected = [0u8; FB_PIXELS];
        let mut actual = [0u8; FB_PIXELS];
        actual[0] = 63; // 一个像素不同

        let diff = compare_framebuffers(&expected, &actual);
        let png = diff_to_image(&diff, &expected, &actual, DiffImageLayout::DiffOnly);
        assert!(!png.is_empty());
        // PNG 头部标志
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn test_diff_to_image_side_by_side() {
        let expected = [0u8; FB_PIXELS];
        let actual = [1u8; FB_PIXELS];
        let diff = compare_framebuffers(&expected, &actual);
        let png = diff_to_image(&diff, &expected, &actual, DiffImageLayout::SideBySide);
        assert!(!png.is_empty());
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn test_index_to_rgb() {
        let fb = [0u8; FB_PIXELS];
        let rgb = index_to_rgb(&fb);
        assert_eq!(rgb.len(), FB_PIXELS * 3);
        // 索引 0 对应 (84, 84, 84)
        assert_eq!(rgb[0], 84);
        assert_eq!(rgb[1], 84);
        assert_eq!(rgb[2], 84);
    }
}