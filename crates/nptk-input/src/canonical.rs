/// Canonical (backend-agnostic) gamepad state.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CanonicalGamepadState {
    // Face buttons (SNES / Xbox layout)
    pub south: bool,
    pub east: bool,
    pub west: bool,
    pub north: bool,
    // Shoulder buttons
    pub left_shoulder: bool,
    pub right_shoulder: bool,
    // Analog triggers
    pub left_trigger: f32,
    pub right_trigger: f32,
    // Menu / special
    pub select: bool,
    pub start: bool,
    pub guide: bool,
    // Stick clicks
    pub left_stick_button: bool,
    pub right_stick_button: bool,
    // D-pad
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    // Analog sticks
    pub left_stick: [f32; 2],
    pub right_stick: [f32; 2],
}

// ponytail: simple struct, no builder pattern -- add when 8+ fields need defaults different from false/0.0