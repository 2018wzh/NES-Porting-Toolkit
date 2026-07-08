use serde::{Deserialize, Serialize};

/// The kind of code region a hook describes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookType {
    NamedFunction,
    DataTable,
    JumpTable,
    SkipRegion,
    HardwareRegister,
}

/// A single code-hook definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeHook {
    pub address: u16,
    pub name: String,
    pub hook_type: HookType,
    pub size: Option<u16>,
    pub comment: Option<String>,
}

/// Top-level hook configuration loaded from a TOML / RON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    pub hooks: Vec<CodeHook>,
}
