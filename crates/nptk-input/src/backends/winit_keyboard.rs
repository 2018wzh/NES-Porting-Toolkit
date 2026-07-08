//! winit keyboard backend — maps keyboard keys to canonical gamepad state.
//!
//! This backend wraps `keyboard_to_canonical()` from the parent module and
//! exposes a single virtual "keyboard gamepad" device.  Key events are fed
//! via `handle_key_event()` (called from the winit event loop) and polled
//! with `poll()`.

use crate::backend::{
    keyboard_to_canonical, InputBackend, InputBackendKind, InputDeviceInfo, PhysicalDeviceId,
    RawGamepadState,
};
use crate::canonical::CanonicalGamepadState;

/// winit keyboard backend — single virtual gamepad mapped from keyboard keys.
pub struct WinitKeyboardBackend {
    state: CanonicalGamepadState,
    last_polled_state: CanonicalGamepadState,
    device_id: PhysicalDeviceId,
}

impl WinitKeyboardBackend {
    pub fn new() -> Self {
        WinitKeyboardBackend {
            state: CanonicalGamepadState::default(),
            last_polled_state: CanonicalGamepadState::default(),
            device_id: PhysicalDeviceId {
                backend: InputBackendKind::WinitKeyboard,
                local_id: 0,
            },
        }
    }

    /// Feed a key event from the winit event loop into the keyboard state.
    ///
    /// `key_name` should be the string representation of the key (e.g.
    /// `"ArrowUp"`, `"KeyZ"`, `"Enter"`).  `pressed` indicates key-down vs
    /// key-up.
    pub fn handle_key_event(&mut self, key_name: &str, pressed: bool) {
        keyboard_to_canonical(key_name, pressed, &mut self.state);
    }

    /// Reset all keys to released (useful when the window loses focus).
    pub fn release_all(&mut self) {
        self.state = CanonicalGamepadState::default();
    }

    /// Access the current canonical state (for merging with gamepad input).
    pub fn state(&self) -> &CanonicalGamepadState {
        &self.state
    }
}

impl Default for WinitKeyboardBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBackend for WinitKeyboardBackend {
    fn kind(&self) -> InputBackendKind {
        InputBackendKind::WinitKeyboard
    }

    fn poll(&mut self, now_ns: u64, sink: &mut dyn crate::backend::InputEventSink) {
        // Only emit if state changed since last poll
        if self.state != self.last_polled_state {
            self.last_polled_state = self.state.clone();

            let raw = RawGamepadState {
                device_id: self.device_id,
                name: "Keyboard (winit)".into(),
                buttons: vec![
                    self.state.south,
                    self.state.east,
                    self.state.west,
                    self.state.north,
                    self.state.left_shoulder,
                    self.state.right_shoulder,
                    self.state.select,
                    self.state.start,
                    self.state.guide,
                    self.state.left_stick_button,
                    self.state.right_stick_button,
                    self.state.dpad_up,
                    self.state.dpad_down,
                    self.state.dpad_left,
                    self.state.dpad_right,
                ],
                axes: vec![
                    self.state.left_trigger,
                    self.state.right_trigger,
                    self.state.left_stick[0],
                    self.state.left_stick[1],
                    self.state.right_stick[0],
                    self.state.right_stick[1],
                ],
                timestamp_ns: now_ns,
            };

            sink.on_raw_gamepad(raw);
        }
    }

    fn connected_devices(&self) -> Vec<InputDeviceInfo> {
        vec![InputDeviceInfo {
            device_id: self.device_id,
            name: "Keyboard (winit)".into(),
            vendor_id: None,
            product_id: None,
            backend: InputBackendKind::WinitKeyboard,
        }]
    }

    fn set_rumble(&mut self, _device: PhysicalDeviceId, _low: f32, _high: f32) -> Result<(), ()> {
        Err(()) // keyboard has no rumble
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyboard_backend_default() {
        let backend = WinitKeyboardBackend::new();
        assert_eq!(backend.kind(), InputBackendKind::WinitKeyboard);
        assert_eq!(backend.connected_devices().len(), 1);
    }

    #[test]
    fn test_key_press_updates_state() {
        let mut backend = WinitKeyboardBackend::new();
        backend.handle_key_event("ArrowUp", true);
        assert!(backend.state.dpad_up);

        backend.handle_key_event("ArrowUp", false);
        assert!(!backend.state.dpad_up);
    }

    #[test]
    fn test_release_all() {
        let mut backend = WinitKeyboardBackend::new();
        backend.handle_key_event("z", true);
        backend.handle_key_event("ArrowRight", true);
        assert!(backend.state.south);
        assert!(backend.state.dpad_right);

        backend.release_all();
        assert!(!backend.state.south);
        assert!(!backend.state.dpad_right);
    }
}
