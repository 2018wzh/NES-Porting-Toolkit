//! # nptk-verify — NES 移植验证工具
//!
//! 提供帧缓冲比较、差异分析和多帧对比会话功能，
//! 用于验证原生移植（重编译 + WGPU 渲染）与参考实现（解释器 + 软件渲染）的行为一致性。
//!
//! ## 快速开始
//!
//! ```ignore
//! use nptk_verify::compare::compare_framebuffers;
//! use nptk_verify::report::{ComparisonSession, VerifyMode, create_session_from_rom};
//!
//! // 从 ROM 创建对比会话
//! let mut session = create_session_from_rom("rom.nes", VerifyMode::InterpreterVsRecompiled)?;
//!
//! // 运行 60 帧对比
//! let report = session.run_frames(60, None);
//!
//! // 输出摘要
//! println!("{}", report.summary());
//!
//! // 写入差异图像
//! report.write_diff_images(std::path::Path::new("verify_output"), session.ref_frames(), session.actual_frames())?;
//! ```

pub mod compare;
pub mod report;
