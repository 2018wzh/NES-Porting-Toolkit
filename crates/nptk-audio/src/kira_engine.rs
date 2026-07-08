//! Kira native audio engine — plays SFX/BGM events natively.
//!
//! Integrates with the Kira audio library for low-latency game audio playback.
//! Supports SFX (one-shot sounds) and BGM (looping background music).
//!
//! ## Architecture
//!
//! ```text
//! NativeAudioEvent
//!   → KiraEngine::dispatch()
//!     → Kira AudioManager
//!       → mixer → output stream
//! ```
//!
//! SFX sounds are loaded as one-shot `StaticSoundData` instances.
//! BGM is loaded as a looping `StaticSoundData`. Volume is controlled
//! via a master track.
//!
//! ## Asset loading
//!
//! Audio assets (WAV/OGG files) are loaded via `StaticSoundData::from_file()`.
//! The engine maintains a cache of loaded sounds keyed by `SfxId`/`BgmId`.
//! Asset paths are resolved relative to the configured assets directory.

use crate::kira_events::{NativeAudioEvent, SfxId, BgmId};
use kira::{
    manager::{AudioManager, AudioManagerSettings},
    sound::static_sound::{StaticSoundData, StaticSoundSettings, StaticSoundHandle},
    track::{TrackBuilder, TrackHandle},
    tween::Tween,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// Kira native audio engine.
///
/// Manages a Kira `AudioManager` instance and dispatches
/// `NativeAudioEvent`s to actual audio playback.
pub struct KiraEngine {
    /// Kira audio manager (initialized on first use).
    manager: Option<AudioManager>,
    /// Master track handle for volume control.
    master_track: Option<TrackHandle>,
    /// Loaded SFX sounds, keyed by SfxId.
    sfx_cache: HashMap<SfxId, StaticSoundData>,
    /// Active SFX sound handles (for stop/control).
    active_sfx: HashMap<SfxId, StaticSoundHandle>,
    /// Loaded BGM sound data.
    bgm_data: Option<StaticSoundData>,
    /// Active BGM sound handle.
    active_bgm: Option<StaticSoundHandle>,
    /// Base path for audio assets.
    assets_path: PathBuf,
    /// Whether the engine has been successfully initialized.
    initialized: bool,
    /// Buffered events (for testing/debugging).
    pending_events: Vec<NativeAudioEvent>,
}

impl KiraEngine {
    /// Create a new KiraEngine with the given assets path.
    ///
    /// The engine is lazily initialized — `AudioManager::new()` is called
    /// on the first `dispatch()` call. This avoids blocking during setup.
    pub fn new(assets_path: PathBuf) -> Self {
        Self {
            manager: None,
            master_track: None,
            sfx_cache: HashMap::new(),
            active_sfx: HashMap::new(),
            bgm_data: None,
            active_bgm: None,
            assets_path,
            initialized: false,
            pending_events: Vec::new(),
        }
    }

    /// Initialize the Kira AudioManager.
    ///
    /// Returns `true` if initialization succeeded, `false` otherwise.
    /// Failure is non-fatal — the engine will fall back to event buffering.
    fn init(&mut self) -> bool {
        if self.initialized {
            return true;
        }

        match AudioManager::new(AudioManagerSettings::default()) {
            Ok(mut manager) => {
                let track = manager.add_sub_track(TrackBuilder::default());
                match track {
                    Ok(track_handle) => {
                        self.manager = Some(manager);
                        self.master_track = Some(track_handle);
                        self.initialized = true;
                        tracing::info!("Kira audio engine initialized successfully");
                        true
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create Kira sub-track: {:?}", e);
                        false
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to initialize Kira AudioManager: {:?}", e);
                false
            }
        }
    }

    /// Load an SFX sound file into the cache.
    ///
    /// The file is expected at `{assets_path}/sfx/{id}.wav` or `{id}.ogg`.
    pub fn load_sfx(&mut self, id: SfxId) {
        if self.sfx_cache.contains_key(&id) {
            return; // Already loaded
        }

        let name = match id {
            SfxId::Shoot => "shoot",
            SfxId::Explosion => "explosion",
            SfxId::PowerUp => "powerup",
            SfxId::GameOver => "gameover",
        };

        // Try .wav first, then .ogg
        let paths = [
            self.assets_path.join("sfx").join(format!("{}.wav", name)),
            self.assets_path.join("sfx").join(format!("{}.ogg", name)),
        ];

        for path in &paths {
            if path.exists() {
                match StaticSoundData::from_file(path) {
                    Ok(data) => {
                        self.sfx_cache.insert(id, data);
                        tracing::debug!("Loaded SFX {:?} from {:?}", id, path);
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("Failed to load SFX {:?} from {:?}: {:?}", id, path, e);
                    }
                }
            }
        }

        tracing::warn!("SFX {:?} not found at {:?}", id, self.assets_path.join("sfx"));
    }

    /// Load a BGM sound file.
    ///
    /// The file is expected at `{assets_path}/bgm/{id}.wav` or `{id}.ogg`.
    pub fn load_bgm(&mut self, id: BgmId) {
        let name = match id {
            BgmId::Stage1 => "stage1",
            BgmId::StageClear => "stage_clear",
        };

        let paths = [
            self.assets_path.join("bgm").join(format!("{}.wav", name)),
            self.assets_path.join("bgm").join(format!("{}.ogg", name)),
        ];

        for path in &paths {
            if path.exists() {
                match StaticSoundData::from_file(path) {
                    Ok(data) => {
                        self.bgm_data = Some(data);
                        tracing::debug!("Loaded BGM {:?} from {:?}", id, path);
                        return;
                    }
                    Err(e) => {
                        tracing::debug!("Failed to load BGM {:?} from {:?}: {:?}", id, path, e);
                    }
                }
            }
        }

        tracing::warn!("BGM {:?} not found at {:?}", id, self.assets_path.join("bgm"));
    }

    /// Dispatch a native audio event for playback.
    ///
    /// If Kira is not initialized, the event is buffered.
    /// If initialization fails, the event is still buffered for debugging.
    pub fn dispatch(&mut self, event: NativeAudioEvent) {
        // Try to initialize on first use
        if !self.initialized {
            self.init();
        }

        match event {
            NativeAudioEvent::PlaySfx { id } => {
                tracing::debug!("Kira SFX: {:?}", id);
                // Load on demand (separate borrow from manager)
                if !self.sfx_cache.contains_key(&id) {
                    self.load_sfx(id);
                }
                // Play via manager (separate scope for borrow)
                if let Some(data) = self.sfx_cache.get(&id).cloned() {
                    if let Some(manager) = &mut self.manager {
                        match manager.play(data) {
                            Ok(handle) => {
                                self.active_sfx.insert(id, handle);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to play SFX {:?}: {:?}", id, e);
                            }
                        }
                    }
                }
            }
            NativeAudioEvent::StopSfx => {
                tracing::debug!("Kira SFX: stop all");
                self.active_sfx.clear();
            }
            NativeAudioEvent::PlayBgm { id } => {
                tracing::debug!("Kira BGM: {:?}", id);
                // Stop current BGM
                if let Some(mut handle) = self.active_bgm.take() {
                    let _ = handle.stop(Tween::default());
                }
                // Load on demand
                self.load_bgm(id);
                // Play via manager
                if let Some(data) = self.bgm_data.clone() {
                    let settings = StaticSoundSettings::new().loop_region(..);
                    let mut loop_data = data;
                    loop_data.settings = settings;
                    if let Some(manager) = &mut self.manager {
                        match manager.play(loop_data) {
                            Ok(handle) => {
                                self.active_bgm = Some(handle);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to play BGM {:?}: {:?}", id, e);
                            }
                        }
                    }
                }
            }
            NativeAudioEvent::StopBgm => {
                tracing::debug!("Kira BGM: stop");
                if let Some(mut handle) = self.active_bgm.take() {
                    let _ = handle.stop(Tween::default());
                }
            }
            NativeAudioEvent::SetVolume { value } => {
                tracing::debug!("Kira volume: {}", value);
                if let Some(track) = &mut self.master_track {
                    let _ = track.set_volume(value as f64, Tween::default());
                }
            }
        }

        self.pending_events.push(event);
    }

    /// Drain pending events (for testing/debugging).
    pub fn drain_events(&mut self) -> Vec<NativeAudioEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Check if the engine is initialized and ready.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Default for KiraEngine {
    fn default() -> Self {
        Self::new(PathBuf::from("assets"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kira_events::{SfxId, BgmId};

    #[test]
    fn test_dispatch_sfx() {
        let mut engine = KiraEngine::new(PathBuf::from("assets"));
        engine.dispatch(NativeAudioEvent::PlaySfx { id: SfxId::Shoot });
        let events = engine.drain_events();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_dispatch_bgm() {
        let mut engine = KiraEngine::new(PathBuf::from("assets"));
        engine.dispatch(NativeAudioEvent::PlayBgm { id: BgmId::Stage1 });
        let events = engine.drain_events();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_stop_sfx() {
        let mut engine = KiraEngine::new(PathBuf::from("assets"));
        engine.dispatch(NativeAudioEvent::PlaySfx { id: SfxId::Shoot });
        engine.dispatch(NativeAudioEvent::StopSfx);
        let events = engine.drain_events();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_stop_bgm() {
        let mut engine = KiraEngine::new(PathBuf::from("assets"));
        engine.dispatch(NativeAudioEvent::PlayBgm { id: BgmId::Stage1 });
        engine.dispatch(NativeAudioEvent::StopBgm);
        let events = engine.drain_events();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_set_volume() {
        let mut engine = KiraEngine::new(PathBuf::from("assets"));
        engine.dispatch(NativeAudioEvent::SetVolume { value: 0.5 });
        let events = engine.drain_events();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_not_initialized_without_audio_device() {
        // In CI/headless environments, Kira may not initialize.
        // The engine should handle this gracefully.
        let mut engine = KiraEngine::new(PathBuf::from("assets"));
        assert!(!engine.is_initialized());
        // Dispatch should still buffer events even without init
        engine.dispatch(NativeAudioEvent::PlaySfx { id: SfxId::Shoot });
        assert_eq!(engine.drain_events().len(), 1);
    }
}
