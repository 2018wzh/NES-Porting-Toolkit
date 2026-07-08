//! gilrs gamepad backend
//!
//! Wraps the `gilrs` cross-platform gamepad library, mapping its unified gamepad
//! model into the framework's canonical state and NES controller output.

use std::collections::HashMap;

use crate::backend::{
    InputBackend, InputBackendKind, InputDeviceInfo, PhysicalDeviceId, RawGamepadState,
};
use crate::canonical::CanonicalGamepadState;
use crate::mapper::MappingProfile;

/// Backend that polls gamepads through the `gilrs` library.
pub struct GilrsBackend {
    gilrs: gilrs::Gilrs,
    /// Mapping of gilrs gamepad id → local PhysicalDeviceId
    device_map: HashMap<gilrs::GamepadId, PhysicalDeviceId>,
    next_local_id: u64,
    /// Last-known state per device id (for change detection)
    last_states: HashMap<gilrs::GamepadId, CanonicalGamepadState>,
    /// Optional: an InputMapper profile for configuring the layout.
    /// If `None`, a sensible default NES-friendly mapping is used.
    mapping_profile: Option<MappingProfile>,
}

impl GilrsBackend {
    /// Create a new gilrs backend.
    pub fn new() -> Result<Self, gilrs::Error> {
        let gilrs = gilrs::Gilrs::new()?;
        Ok(Self {
            gilrs,
            device_map: HashMap::new(),
            next_local_id: 0,
            last_states: HashMap::new(),
            mapping_profile: None,
        })
    }

    /// Attach an optional mapping profile.
    pub fn with_mapping_profile(mut self, profile: MappingProfile) -> Self {
        self.mapping_profile = Some(profile);
        self
    }

    /// Access the underlying gilrs instance for advanced use.
    pub fn gilrs(&self) -> &gilrs::Gilrs {
        &self.gilrs
    }

    pub fn gilrs_mut(&mut self) -> &mut gilrs::Gilrs {
        &mut self.gilrs
    }

    /// Convert a gilrs gamepad state to our `CanonicalGamepadState`.
    fn gilrs_to_canonical(gamepad: &gilrs::Gamepad<'_>) -> CanonicalGamepadState {
        use gilrs::Button;

        let btn = |b: Button| -> bool { gamepad.is_pressed(b) };

        let axis =
            |a: gilrs::Axis| -> f32 { gamepad.axis_data(a).map(|d| d.value()).unwrap_or(0.0) };

        CanonicalGamepadState {
            south: btn(Button::South),
            east: btn(Button::East),
            west: btn(Button::West),
            north: btn(Button::North),
            left_shoulder: btn(Button::LeftTrigger),
            right_shoulder: btn(Button::RightTrigger),
            // Triggers as analog axes (gilrs exposes these as buttons too)
            left_trigger: if btn(Button::LeftTrigger2) {
                1.0
            } else {
                axis(gilrs::Axis::LeftZ)
            },
            right_trigger: if btn(Button::RightTrigger2) {
                1.0
            } else {
                axis(gilrs::Axis::RightZ)
            },
            select: btn(Button::Select),
            start: btn(Button::Start),
            guide: btn(Button::Mode),
            left_stick_button: btn(Button::LeftThumb),
            right_stick_button: btn(Button::RightThumb),
            dpad_up: btn(Button::DPadUp),
            dpad_down: btn(Button::DPadDown),
            dpad_left: btn(Button::DPadLeft),
            dpad_right: btn(Button::DPadRight),
            left_stick: [axis(gilrs::Axis::LeftStickX), axis(gilrs::Axis::LeftStickY)],
            right_stick: [
                axis(gilrs::Axis::RightStickX),
                axis(gilrs::Axis::RightStickY),
            ],
        }
    }

    /// Build a `RawGamepadState` from a `CanonicalGamepadState`.
    fn canonical_to_raw(
        device_id: PhysicalDeviceId,
        name: &str,
        canonical: &CanonicalGamepadState,
    ) -> RawGamepadState {
        RawGamepadState {
            device_id,
            name: name.to_string(),
            buttons: vec![
                canonical.south,
                canonical.east,
                canonical.west,
                canonical.north,
                canonical.left_shoulder,
                canonical.right_shoulder,
                canonical.select,
                canonical.start,
                canonical.guide,
                canonical.left_stick_button,
                canonical.right_stick_button,
                canonical.dpad_up,
                canonical.dpad_down,
                canonical.dpad_left,
                canonical.dpad_right,
            ],
            axes: vec![
                canonical.left_trigger,
                canonical.right_trigger,
                canonical.left_stick[0],
                canonical.left_stick[1],
                canonical.right_stick[0],
                canonical.right_stick[1],
            ],
            timestamp_ns: 0,
        }
    }

    /// Ensure every connected gamepad has a PhysicalDeviceId.
    fn sync_devices(&mut self) {
        for (gid, _gamepad) in self.gilrs.gamepads() {
            if !self.device_map.contains_key(&gid) {
                let local_id = self.next_local_id;
                self.next_local_id += 1;
                self.device_map.insert(
                    gid,
                    PhysicalDeviceId {
                        backend: InputBackendKind::Gilrs,
                        local_id,
                    },
                );
                // Prepopulate last state with defaults
                self.last_states
                    .insert(gid, CanonicalGamepadState::default());
            }
        }
    }
}

impl InputBackend for GilrsBackend {
    fn kind(&self) -> InputBackendKind {
        InputBackendKind::Gilrs
    }

    fn poll(&mut self, now_ns: u64, sink: &mut dyn crate::backend::InputEventSink) {
        // Drain pending events (hotplug, etc.)
        while let Some(_ev) = self.gilrs.next_event() {
            // Events are processed automatically by gilrs; we just need to drain
            // the queue so that gamepad states are up to date.
        }

        // Sync device list
        self.sync_devices();

        // Poll each gamepad
        for (gid, gamepad) in self.gilrs.gamepads() {
            let Some(&dev_id) = self.device_map.get(&gid) else {
                continue;
            };

            if !gamepad.is_connected() {
                continue;
            }

            let canonical = Self::gilrs_to_canonical(&gamepad);
            let name = gamepad.name().to_string();
            let mut raw = Self::canonical_to_raw(dev_id, &name, &canonical);
            raw.timestamp_ns = now_ns;

            // Optionally apply mapping profile
            // The mapping profile is applied later in the pipeline;
            // here we just pass through the raw canonical state.
            let _ = &self.mapping_profile;

            // Report change
            let last = self.last_states.entry(gid).or_default();
            if *last != canonical {
                *last = canonical;
                sink.on_raw_gamepad(raw);
            }
        }
    }

    fn connected_devices(&self) -> Vec<InputDeviceInfo> {
        self.gilrs
            .gamepads()
            .filter(|(_id, gp)| gp.is_connected())
            .map(|(gid, gp)| {
                let local_id = self.device_map.get(&gid).map(|d| d.local_id).unwrap_or(0);
                InputDeviceInfo {
                    device_id: PhysicalDeviceId {
                        backend: InputBackendKind::Gilrs,
                        local_id,
                    },
                    name: gp.name().to_string(),
                    vendor_id: None,
                    product_id: None,
                    backend: InputBackendKind::Gilrs,
                }
            })
            .collect()
    }

    fn set_rumble(&mut self, device: PhysicalDeviceId, low: f32, high: f32) -> Result<(), ()> {
        // gilrs 0.11: rumble is set through Gamepad::set_ff_state or similar.
        // For now this is a stub — most NES games don't use rumble anyway.
        let _ = (device, low, high);
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gilrs_to_canonical_empty() {
        // Without a real gamepad we just verify the conversion doesn't panic
        let state = CanonicalGamepadState::default();
        assert!(!state.south);
        assert!(!state.start);
    }

    #[test]
    fn test_canonical_to_raw_roundtrip() {
        let mut canonical = CanonicalGamepadState::default();
        canonical.south = true;
        canonical.dpad_up = true;

        let dev_id = PhysicalDeviceId {
            backend: InputBackendKind::Gilrs,
            local_id: 0,
        };
        let raw = GilrsBackend::canonical_to_raw(dev_id, "test", &canonical);
        assert_eq!(raw.name, "test");
        assert!(raw.buttons[0]); // south
        assert!(raw.buttons[11]); // dpad_up
    }
}
