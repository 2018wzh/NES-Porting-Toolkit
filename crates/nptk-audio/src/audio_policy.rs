//! Audio policy — routes audio between compatible APU output and native Kira events.
//!
//! ## Routing Modes
//!
//! | Mode | Description |
//! |---|---|
//! | `ApuCompat` | APU compat → CPAL PCM output (cycle-accurate emulation) |
//! | `NativeSfx` | APU compat BGM + Kira SFX events for sound effects |
//! | `NativeFull` | Kira BGM + Kira SFX (fully native audio) |
//!
//! The policy is loaded from the `[audio]` section of the GameProfile.

use crate::kira_events::NativeAudioEvent;
#[cfg(test)]
use crate::kira_events::SfxId;

/// Audio routing policy — determines how audio is processed.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioPolicy {
    /// Full APU compatibility mode: all audio goes through the APU emulation
    /// and is output as PCM via CPAL.
    ApuCompat,

    /// Hybrid mode: background music uses APU compat, but sound effects are
    /// intercepted and played natively through Kira.
    NativeSfx,

    /// Full native mode: both BGM and SFX are handled by Kira, not the APU.
    NativeFull,
}

impl Default for AudioPolicy {
    fn default() -> Self {
        AudioPolicy::ApuCompat
    }
}

/// Audio routing engine — decides how audio events are dispatched.
pub struct AudioRouter {
    pub policy: AudioPolicy,
    /// Accumulated APU PCM samples for the current frame.
    pub apu_samples: Vec<f32>,
    /// Queued native audio events for this frame.
    pub native_events: Vec<NativeAudioEvent>,
}

impl AudioRouter {
    pub fn new(policy: AudioPolicy) -> Self {
        AudioRouter {
            policy,
            apu_samples: Vec::with_capacity(4096),
            native_events: Vec::new(),
        }
    }

    /// Push a raw APU PCM sample (from ApuMixer).
    pub fn push_apu_sample(&mut self, sample: f32) {
        self.apu_samples.push(sample);
    }

    /// Queue a native audio event (e.g., PlaySfx).
    pub fn queue_event(&mut self, event: NativeAudioEvent) {
        self.native_events.push(event);
    }

    /// Drain all accumulated APU samples and return them.
    pub fn drain_apu_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.apu_samples)
    }

    /// Drain all queued native events and return them.
    pub fn drain_events(&mut self) -> Vec<NativeAudioEvent> {
        std::mem::take(&mut self.native_events)
    }

    /// Whether to use APU compat for BGM.
    pub fn use_apu_bgm(&self) -> bool {
        matches!(self.policy, AudioPolicy::ApuCompat | AudioPolicy::NativeSfx)
    }

    /// Whether to use native SFX via Kira.
    pub fn use_native_sfx(&self) -> bool {
        matches!(
            self.policy,
            AudioPolicy::NativeSfx | AudioPolicy::NativeFull
        )
    }

    /// Whether to use native BGM via Kira.
    pub fn use_native_bgm(&self) -> bool {
        matches!(self.policy, AudioPolicy::NativeFull)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_apu_compat() {
        let router = AudioRouter::new(AudioPolicy::default());
        assert!(router.use_apu_bgm());
        assert!(!router.use_native_sfx());
        assert!(!router.use_native_bgm());
    }

    #[test]
    fn test_native_sfx_mode() {
        let router = AudioRouter::new(AudioPolicy::NativeSfx);
        assert!(router.use_apu_bgm());
        assert!(router.use_native_sfx());
        assert!(!router.use_native_bgm());
    }

    #[test]
    fn test_native_full_mode() {
        let router = AudioRouter::new(AudioPolicy::NativeFull);
        assert!(!router.use_apu_bgm());
        assert!(router.use_native_sfx());
        assert!(router.use_native_bgm());
    }

    #[test]
    fn test_sample_drain() {
        let mut router = AudioRouter::new(AudioPolicy::ApuCompat);
        router.push_apu_sample(0.5);
        router.push_apu_sample(-0.3);
        let samples = router.drain_apu_samples();
        assert_eq!(samples, vec![0.5, -0.3]);
        assert!(router.drain_apu_samples().is_empty());
    }

    #[test]
    fn test_event_queue_drain() {
        let mut router = AudioRouter::new(AudioPolicy::NativeSfx);
        router.queue_event(NativeAudioEvent::PlaySfx { id: SfxId::Shoot });
        let events = router.drain_events();
        assert_eq!(events.len(), 1);
        assert!(router.drain_events().is_empty());
    }
}
