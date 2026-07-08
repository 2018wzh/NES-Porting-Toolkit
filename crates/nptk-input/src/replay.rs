/// Deterministic input replay backend.
///
/// `InputReplay` stores a sequence of frames; each frame lists the button
/// names active on port 1 (and optionally port 2).  The `ReplayBackend`
/// wraps this and provides `state_for_frame()` / `advance_frame()`.
use serde::{Deserialize, Serialize};

use crate::nes_controller::NesControllerState;

/// Input state for a single frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputReplayFrame {
    pub frame: u64,
    /// Active button names on port 1 (e.g. `"A"`, `"B"`, `"UP"`).
    pub port1: Vec<String>,
    /// Active button names on port 2.
    pub port2: Option<Vec<String>>,
}

/// A complete input replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputReplay {
    pub version: u8,
    pub fps: u8,
    pub frames: Vec<InputReplayFrame>,
}

/// Parses a single button-name string like `"A"` or `"LEFT"` into a
/// `NesControllerState` and sets the appropriate field.
fn apply_button_name(name: &str, state: &mut NesControllerState) {
    match name {
        "A" | "a" => state.a = true,
        "B" | "b" => state.b = true,
        "SELECT" | "select" => state.select = true,
        "START" | "start" => state.start = true,
        "UP" | "up" => state.up = true,
        "DOWN" | "down" => state.down = true,
        "LEFT" | "left" => state.left = true,
        "RIGHT" | "right" => state.right = true,
        _ => {} // unknown name is silently ignored
    }
}

fn frame_to_state(frame: &InputReplayFrame, port: u8) -> NesControllerState {
    let names = match port {
        1 => &frame.port1,
        2 => frame.port2.as_ref().unwrap_or(&frame.port1),
        _ => return NesControllerState::default(),
    };
    let mut s = NesControllerState::default();
    for name in names {
        apply_button_name(name, &mut s);
    }
    s
}

/// Backend that serves pre-recorded input frame-by-frame.
#[derive(Debug, Clone)]
pub struct ReplayBackend {
    replay: InputReplay,
    current_frame: u64,
}

impl ReplayBackend {
    pub fn new(replay: InputReplay) -> Self {
        Self {
            replay,
            current_frame: 0,
        }
    }

    /// Return the `NesControllerState` for the given absolute frame number
    /// on the given port (1 or 2).
    ///
    /// If no replay frame exactly matches the requested frame the last frame
    /// whose `frame` <= the requested frame is used (previous-frame hold).
    /// Returns default state when there is no matching frame at all.
    pub fn state_for_frame(&self, frame: u64, port: u8) -> NesControllerState {
        let mut best: Option<&InputReplayFrame> = None;
        for f in &self.replay.frames {
            if f.frame <= frame {
                best = Some(f);
            } else {
                break;
            }
        }
        match best {
            Some(f) => frame_to_state(f, port),
            None => NesControllerState::default(),
        }
    }

    /// Advance the internal frame counter by 1 and return the new state for port 1.
    pub fn advance_frame(&mut self) -> NesControllerState {
        let state = self.state_for_frame(self.current_frame, 1);
        self.current_frame += 1;
        state
    }

    /// Reset the internal counter back to 0.
    pub fn reset(&mut self) {
        self.current_frame = 0;
    }

    /// The raw replay data (read-only).
    pub fn replay(&self) -> &InputReplay {
        &self.replay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_replay() -> InputReplay {
        InputReplay {
            version: 1,
            fps: 60,
            frames: vec![
                InputReplayFrame {
                    frame: 0,
                    port1: vec!["A".into(), "START".into()],
                    port2: None,
                },
                InputReplayFrame {
                    frame: 2,
                    port1: vec!["B".into(), "UP".into()],
                    port2: Some(vec!["LEFT".into()]),
                },
            ],
        }
    }

    #[test]
    fn frame_0_state() {
        let backend = ReplayBackend::new(test_replay());
        let s = backend.state_for_frame(0, 1);
        assert!(s.a);
        assert!(s.start);
        assert!(!s.b);
    }

    #[test]
    fn frame_1_holds_previous() {
        let backend = ReplayBackend::new(test_replay());
        let s = backend.state_for_frame(1, 1);
        // frame 1 has no explicit entry; frame 0 carries forward
        assert!(s.a);
        assert!(s.start);
    }

    #[test]
    fn port_2_frame_2() {
        let backend = ReplayBackend::new(test_replay());
        let s = backend.state_for_frame(2, 2);
        assert!(s.left);
        assert!(!s.right);
    }

    #[test]
    fn advance() {
        let mut backend = ReplayBackend::new(test_replay());
        let s0 = backend.advance_frame(); // frame 0
        assert!(s0.a);
        let s1 = backend.advance_frame(); // frame 1 (held from frame 0)
        assert!(s1.a);
        let s2 = backend.advance_frame(); // frame 2
        assert!(!s2.a);
        assert!(s2.b);
        assert!(s2.up);
    }

    #[test]
    fn reset() {
        let mut backend = ReplayBackend::new(test_replay());
        backend.advance_frame();
        backend.advance_frame();
        backend.reset();
        let s = backend.state_for_frame(0, 1);
        assert!(s.a);
    }
}
