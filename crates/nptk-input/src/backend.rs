/// Input backend trait -- all backends implement this
use crate::canonical::CanonicalGamepadState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputBackendKind {
    WinitKeyboard,
    Gilrs,
    HidApi,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalDeviceId {
    pub backend: InputBackendKind,
    pub local_id: u64,
}

#[derive(Debug, Clone)]
pub struct InputDeviceInfo {
    pub device_id: PhysicalDeviceId,
    pub name: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub backend: InputBackendKind,
}

#[derive(Debug, Clone)]
pub struct RawGamepadState {
    pub device_id: PhysicalDeviceId,
    pub name: String,
    pub buttons: Vec<bool>,
    pub axes: Vec<f32>,
    pub timestamp_ns: u64,
}

pub trait InputEventSink {
    fn on_raw_gamepad(&mut self, state: RawGamepadState);
    fn on_device_connected(&mut self, info: InputDeviceInfo);
    fn on_device_disconnected(&mut self, id: PhysicalDeviceId);
}

pub trait InputBackend {
    fn kind(&self) -> InputBackendKind;
    fn poll(&mut self, now_ns: u64, sink: &mut dyn InputEventSink);
    fn connected_devices(&self) -> Vec<InputDeviceInfo>;
    fn set_rumble(&mut self, device: PhysicalDeviceId, low: f32, high: f32) -> Result<(), ()>;
}

/// Map a named keypress into a CanonicalGamepadState.
///
/// Recognises WASD / arrow keys for the dpad, Z / X for South / East
/// (NES A / B convention), Enter / Space for Start, RShift for Select.
pub fn keyboard_to_canonical(key_name: &str, pressed: bool, state: &mut CanonicalGamepadState) {
    let v = pressed;
    match key_name {
        // D-pad
        "w" | "W" | "ArrowUp"    => state.dpad_up = v,
        "s" | "S" | "ArrowDown"  => state.dpad_down = v,
        "a" | "A" | "ArrowLeft"  => state.dpad_left = v,
        "d" | "D" | "ArrowRight" => state.dpad_right = v,
        // NES action buttons (Z=A / X=B)
        "z" | "Z" => state.south = v,
        "x" | "X" => state.east = v,
        // Start / Select
        "Enter"      => state.start = v,
        " "          => state.start = v,
        "RShift"     => state.select = v,
        // Shoulder / triggers (mapped to Q / E for convenience)
        "q" | "Q" => state.left_shoulder = v,
        "e" | "E" => state.right_shoulder = v,
        // Guide / stick buttons (sensible defaults)
        "Tab"             => state.guide = v,
        "LControl"        => state.left_stick_button = v,
        "RControl"        => state.right_stick_button = v,
        // Face buttons W / N (NES-style alternate mapping)
        "n" | "N" => state.west = v,
        "m" | "M" => state.north = v,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasd_maps_dpad() {
        let mut s = CanonicalGamepadState::default();
        keyboard_to_canonical("w", true, &mut s);
        assert!(s.dpad_up);
        keyboard_to_canonical("ArrowDown", true, &mut s);
        assert!(s.dpad_down);
    }

    #[test]
    fn zx_maps_ab() {
        let mut s = CanonicalGamepadState::default();
        keyboard_to_canonical("z", true, &mut s);
        assert!(s.south);
        keyboard_to_canonical("X", true, &mut s);
        assert!(s.east);
    }
}