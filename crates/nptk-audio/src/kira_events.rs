//! Kira 原生音频事件

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SfxId {
    Shoot,
    Explosion,
    PowerUp,
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BgmId {
    Stage1,
    StageClear,
}

#[derive(Debug, Clone)]
pub enum NativeAudioEvent {
    PlaySfx { id: SfxId },
    StopSfx,
    PlayBgm { id: BgmId },
    StopBgm,
    SetVolume { value: f32 },
}