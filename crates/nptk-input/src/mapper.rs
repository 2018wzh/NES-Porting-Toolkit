/// `InputMapper` maps physical button/axis names to canonical fields.
///
/// A mapping profile maps an input-name string (e.g. `"BTN_SOUTH"`,
/// `"ABS_X"`) to a canonical field index.  Profiles can be serialised
/// (via serde) and saved/loaded for user-configurable layouts.

use serde::{Deserialize, Serialize};

use crate::backend::RawGamepadState;
use crate::canonical::CanonicalGamepadState;

/// Indices into `CanonicalGamepadState` for the fields that are settable
/// from a physical button or axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum CanonicalField {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Select,
    Start,
    Guide,
    LeftStickButton,
    RightStickButton,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
}

/// A single mapping entry: when the physical input named `input_name`
/// changes, apply its value to `target`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingEntry {
    pub input_name: String,
    pub target: CanonicalField,
    /// For analog axes, the raw value at rest (typically 0.0 or 32767.0).
    /// The mapper subtracts this and applies `dead_zone` before writing.
    pub center: f32,
    /// Values within `[-dead_zone, +dead_zone]` of `center` are treated as 0.
    pub dead_zone: f32,
    /// Invert the sign of the mapped value.
    pub invert: bool,
}

impl MappingEntry {
    pub fn new(input_name: impl Into<String>, target: CanonicalField) -> Self {
        Self {
            input_name: input_name.into(),
            target,
            center: 0.0,
            dead_zone: 0.0,
            invert: false,
        }
    }

    pub fn with_center(mut self, center: f32) -> Self {
        self.center = center;
        self
    }

    pub fn with_dead_zone(mut self, dz: f32) -> Self {
        self.dead_zone = dz;
        self
    }

    pub fn with_invert(mut self, invert: bool) -> Self {
        self.invert = invert;
        self
    }
}

/// A named mapping profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingProfile {
    pub name: String,
    pub entries: Vec<MappingEntry>,
}

/// Maps raw physical inputs to a `CanonicalGamepadState` by looking up
/// each input name in an ordered mapping table.
#[derive(Debug, Clone)]
pub struct InputMapper {
    profiles: Vec<MappingProfile>,
    active: usize,
}

impl InputMapper {
    pub fn new() -> Self {
        Self {
            profiles: vec![],
            active: 0,
        }
    }

    /// Create a mapper initialised with a single profile.
    pub fn with_profile(profile: MappingProfile) -> Self {
        Self {
            profiles: vec![profile],
            active: 0,
        }
    }

    // -- profile management --

    pub fn profiles(&self) -> &[MappingProfile] {
        &self.profiles
    }

    pub fn add_profile(&mut self, profile: MappingProfile) {
        self.profiles.push(profile);
    }

    pub fn active_profile(&self) -> Option<&MappingProfile> {
        self.profiles.get(self.active)
    }

    pub fn set_active(&mut self, index: usize) -> bool {
        if index < self.profiles.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Apply `raw` onto `target` using the active mapping profile.
    pub fn apply(&self, raw: &RawGamepadState, target: &mut CanonicalGamepadState) {
        let Some(profile) = self.profiles.get(self.active) else {
            return;
        };

        // Build a quick lookup: entry index → button/axis value.
        // Buttons are indexed by position in the buttons vec.
        for entry in &profile.entries {
            let field_val = match self.lookup_value(raw, entry) {
                Some(v) => v,
                None => continue,
            };

            self.set_field(target, entry.target, field_val);
        }
    }

    // -- internal helpers --

    fn lookup_value(&self, raw: &RawGamepadState, entry: &MappingEntry) -> Option<f32> {
        // Try to find the input by name in the buttons list.
        // We store a simple positional heuristic: if the name starts with
        // "BTN_" we look up by index parsed from the suffix, otherwise
        // we fall back to a linear name scan of axes.

        if entry.input_name.starts_with("BTN_") {
            // e.g. "BTN_0", "BTN_12"
            let idx: usize = entry.input_name[4..].parse().ok()?;
            raw.buttons.get(idx).copied().map(|b| if b { 1.0 } else { 0.0 })
        } else if entry.input_name.starts_with("ABS_") || entry.input_name.starts_with("AXIS_") {
            // e.g. "ABS_0", "AXIS_1"
            let idx: usize = entry.input_name[4..].parse().ok()?;
            raw.axes.get(idx).copied().map(|v| {
                let centered = v - entry.center;
                let dz = entry.dead_zone.abs();
                if centered.abs() <= dz {
                    0.0
                } else {
                    let mut out = centered;
                    if entry.invert {
                        out = -out;
                    }
                    // Normalise so that the extreme (±1.0) holds.
                    // If center is non-zero (e.g. 32767.0) assume i16 range.
                    let scale = if entry.center.abs() > 1.0 {
                        entry.center.abs().max(1.0)
                    } else {
                        1.0
                    };
                    (out / scale).clamp(-1.0, 1.0)
                }
            })
        } else {
            // Fallback: linear scan buttons by name.
            // (In a real backend, buttons carry labels; here we use position.)
            None
        }
    }

    fn set_field(&self, target: &mut CanonicalGamepadState, field: CanonicalField, value: f32) {
        let b = value != 0.0;
        match field {
            CanonicalField::South => target.south = b,
            CanonicalField::East => target.east = b,
            CanonicalField::West => target.west = b,
            CanonicalField::North => target.north = b,
            CanonicalField::LeftShoulder => target.left_shoulder = b,
            CanonicalField::RightShoulder => target.right_shoulder = b,
            CanonicalField::LeftTrigger => target.left_trigger = value,
            CanonicalField::RightTrigger => target.right_trigger = value,
            CanonicalField::Select => target.select = b,
            CanonicalField::Start => target.start = b,
            CanonicalField::Guide => target.guide = b,
            CanonicalField::LeftStickButton => target.left_stick_button = b,
            CanonicalField::RightStickButton => target.right_stick_button = b,
            CanonicalField::DpadUp => target.dpad_up = b,
            CanonicalField::DpadDown => target.dpad_down = b,
            CanonicalField::DpadLeft => target.dpad_left = b,
            CanonicalField::DpadRight => target.dpad_right = b,
            CanonicalField::LeftStickX => target.left_stick[0] = value,
            CanonicalField::LeftStickY => target.left_stick[1] = value,
            CanonicalField::RightStickX => target.right_stick[0] = value,
            CanonicalField::RightStickY => target.right_stick[1] = value,
        }
    }
}

impl Default for InputMapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_profile() -> MappingProfile {
        MappingProfile {
            name: "test".into(),
            entries: vec![
                MappingEntry::new("BTN_0", CanonicalField::South),
                MappingEntry::new("BTN_1", CanonicalField::East),
                MappingEntry::new("ABS_0", CanonicalField::LeftStickX)
                    .with_center(0.0)
                    .with_dead_zone(0.1),
            ],
        }
    }

    #[test]
    fn map_buttons() {
        let mapper = InputMapper::with_profile(make_profile());
        let raw = RawGamepadState {
            device_id: crate::backend::PhysicalDeviceId {
                backend: crate::backend::InputBackendKind::Gilrs,
                local_id: 0,
            },
            name: "test".into(),
            buttons: vec![true, false],
            axes: vec![],
            timestamp_ns: 0,
        };
        let mut state = CanonicalGamepadState::default();
        mapper.apply(&raw, &mut state);
        assert!(state.south);
        assert!(!state.east);
    }

    #[test]
    fn map_axis_with_deadzone() {
        let mapper = InputMapper::with_profile(make_profile());
        let raw = RawGamepadState {
            device_id: crate::backend::PhysicalDeviceId {
                backend: crate::backend::InputBackendKind::Gilrs,
                local_id: 0,
            },
            name: "test".into(),
            buttons: vec![],
            axes: vec![0.05], // inside dead zone 0.1
            timestamp_ns: 0,
        };
        let mut state = CanonicalGamepadState::default();
        mapper.apply(&raw, &mut state);
        assert_eq!(state.left_stick[0], 0.0);
    }

    #[test]
    fn no_active_profile_is_noop() {
        let mapper = InputMapper::new();
        let raw = RawGamepadState {
            device_id: crate::backend::PhysicalDeviceId {
                backend: crate::backend::InputBackendKind::Gilrs,
                local_id: 0,
            },
            name: "test".into(),
            buttons: vec![true],
            axes: vec![],
            timestamp_ns: 0,
        };
        let mut state = CanonicalGamepadState::default();
        mapper.apply(&raw, &mut state);
        assert_eq!(state, CanonicalGamepadState::default());
    }
}