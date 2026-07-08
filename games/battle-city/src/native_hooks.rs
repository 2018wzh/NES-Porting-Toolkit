//! Battle City 原生 Hook 注册
//!
//! 将重编译代码中的关键地址标记为 Hook，用于 GameProfile 标注。

use nptk::profile::{CodeHook, HookConfig, HookType};

/// 获取 Battle City 的 Hook 配置
pub fn get_hooks() -> HookConfig {
    HookConfig {
        hooks: vec![
            CodeHook {
                address: 0xE000,
                name: "title_screen_entry".into(),
                hook_type: HookType::NamedFunction,
                size: Some(256),
                comment: Some("Title screen entry point".into()),
            },
            CodeHook {
                address: 0xE100,
                name: "game_init".into(),
                hook_type: HookType::NamedFunction,
                size: Some(512),
                comment: Some("Game initialization".into()),
            },
            CodeHook {
                address: 0xE200,
                name: "player_move_handler".into(),
                hook_type: HookType::NamedFunction,
                size: Some(256),
                comment: Some("Player movement handler".into()),
            },
            CodeHook {
                address: 0xC000,
                name: "stage_data".into(),
                hook_type: HookType::DataTable,
                size: Some(2048),
                comment: Some("Stage layout data".into()),
            },
            CodeHook {
                address: 0xC800,
                name: "level_layouts".into(),
                hook_type: HookType::DataTable,
                size: Some(2048),
                comment: Some("Level layout lookup table".into()),
            },
        ],
    }
}
