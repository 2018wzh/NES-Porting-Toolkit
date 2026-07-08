//! Battle City 语义化游戏状态
//!
//! 将 NES RAM 地址映射为有意义的字段。
//! 参考: https://datacrystal.tcrf.net/wiki/Battle_City_(NES)/RAM_map

/// Battle City 游戏状态视图（只读）
pub struct BattleCityStateView<'a> {
    pub raw_ram: &'a [u8; 0x800],
}

impl<'a> BattleCityStateView<'a> {
    pub fn new(ram: &'a [u8; 0x800]) -> Self {
        Self { raw_ram: ram }
    }

    /// 玩家生命数 ($0051)
    pub fn lives(&self) -> u8 {
        self.raw_ram[0x0051]
    }

    /// 关卡计数器 ($0085)
    pub fn stage_counter(&self) -> u8 {
        self.raw_ram[0x0085]
    }

    /// 跳关标志 ($0080)
    pub fn skip_current_level(&self) -> u8 {
        self.raw_ram[0x0080]
    }

    /// 击杀/道具计数 ($0019)
    pub fn power_counter(&self) -> u8 {
        self.raw_ram[0x0019]
    }

    /// 道具位置 ($0086)
    pub fn power_position(&self) -> u8 {
        self.raw_ram[0x0086]
    }

    /// 道具状态 ($0049)
    pub fn power_status(&self) -> u8 {
        self.raw_ram[0x0049]
    }

    /// 当前坦克状态/方向 ($00A8)
    pub fn current_tank_state(&self) -> u8 {
        self.raw_ram[0x00A8]
    }

    /// 护盾状态 ($0089)
    pub fn shield_status(&self) -> u8 {
        self.raw_ram[0x0089]
    }

    /// 当前方块类型 ($005C)
    pub fn current_block_type(&self) -> u8 {
        self.raw_ram[0x005C]
    }

    /// 玩家 X 坐标 ($00A6)
    pub fn player_x(&self) -> u8 {
        self.raw_ram[0x00A6]
    }

    /// 玩家 Y 坐标 ($00A7)
    pub fn player_y(&self) -> u8 {
        self.raw_ram[0x00A7]
    }

    /// 游戏模式 ($0078): 0=标题, 1=游戏中, 2=Game Over
    pub fn game_mode(&self) -> u8 {
        self.raw_ram[0x0078]
    }

    /// 敌方数量 ($00A1)
    pub fn enemy_count(&self) -> u8 {
        self.raw_ram[0x00A1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_view_reads() {
        let mut ram = [0u8; 0x800];
        ram[0x0051] = 3; // 3 lives
        ram[0x0085] = 5; // stage 5
        ram[0x0078] = 1; // playing

        let state = BattleCityStateView::new(&ram);
        assert_eq!(state.lives(), 3);
        assert_eq!(state.stage_counter(), 5);
        assert_eq!(state.game_mode(), 1);
    }
}
