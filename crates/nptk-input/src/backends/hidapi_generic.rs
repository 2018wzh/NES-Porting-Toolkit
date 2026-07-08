//! HID API backend — generic HID gamepad / joystick support
//!
//! Uses the `hidapi` crate to enumerate and poll HID game controllers that
//! may not be covered by gilrs or XInput.  Useful as a cross-platform
//! fallback for generic USB HID joysticks, arcade sticks, and custom
//! controllers.
//!
//! On Windows this provides the equivalent of a RawInput/DirectInput
//! fallback without needing a native Win32 message loop.

use std::collections::HashMap;

use crate::backend::{
    InputBackend, InputBackendKind, InputDeviceInfo, PhysicalDeviceId, RawGamepadState,
};
use crate::canonical::CanonicalGamepadState;

/// Wrapper that polls HID game controllers.
pub struct HidApiBackend {
    api: Option<hidapi::HidApi>,
    /// Map of HID device path → local PhysicalDeviceId
    device_map: HashMap<String, PhysicalDeviceId>,
    next_local_id: u64,
    /// Last-known canonical state per device for change detection.
    last_states: HashMap<String, CanonicalGamepadState>,
}

impl HidApiBackend {
    /// Create a new HID API backend.  Returns None if `hidapi` fails to
    /// initialise (e.g. no HID subsystem available).
    pub fn new() -> Option<Self> {
        match hidapi::HidApi::new() {
            Ok(api) => Some(Self {
                api: Some(api),
                device_map: HashMap::new(),
                next_local_id: 0,
                last_states: HashMap::new(),
            }),
            Err(_) => None,
        }
    }

    /// Attempt to interpret a HID device as a gamepad and read its state.
    ///
    /// HID gamepads typically expose their button/axis data through input
    /// reports whose format varies wildly across vendors.  This
    /// implementation uses a *lowest-common-denominator* heuristic:
    ///
    /// * Devices with usage-page `0x01` (Generic Desktop) and usage
    ///   `0x04` (Joystick) or `0x05` (Gamepad) are treated as controllers.
    /// * A raw input report is read via `read_timeout`.
    /// * The report bytes are converted to canonical fields using a
    ///   simple positional mapping (first N bytes → buttons, next M
    ///   bytes → axes).
    ///
    /// This is deliberately conservative — full HID report parsing needs
    /// a Report Descriptor parser which is outside the scope of this
    /// backend.
    fn poll_device(
        &self,
        _path: &str,
        dev: &hidapi::HidDevice,
        dev_id: PhysicalDeviceId,
        now_ns: u64,
    ) -> Option<(CanonicalGamepadState, RawGamepadState)> {
        let mut buf = [0u8; 64];
        match dev.read_timeout(&mut buf, 0) {
            Ok(n) if n > 0 => {}
            _ => return None, // no data available
        }

        // Simple heuristic: first 8 bytes → 16 buttons (each bit)
        // bytes 8-15 → 4 axes (2 bytes each, little-endian i16)
        let mut buttons = Vec::with_capacity(16);
        for byte_idx in 0..8.min(buf.len()) {
            let b = buf[byte_idx];
            for bit in 0..8 {
                buttons.push((b & (1 << bit)) != 0);
            }
        }

        let mut axes = Vec::with_capacity(4);
        let axis_base = 8;
        for axis_idx in 0..4 {
            let idx = axis_base + axis_idx * 2;
            if idx + 1 < buf.len() {
                let raw = i16::from_le_bytes([buf[idx], buf[idx + 1]]);
                axes.push((raw as f32) / 32767.0_f32.max(1.0));
            }
        }

        let dpad_up = buttons.get(11).copied().unwrap_or(false);
        let dpad_down = buttons.get(12).copied().unwrap_or(false);
        let dpad_left = buttons.get(13).copied().unwrap_or(false);
        let dpad_right = buttons.get(14).copied().unwrap_or(false);

        let canonical = CanonicalGamepadState {
            south: buttons.get(0).copied().unwrap_or(false),
            east: buttons.get(1).copied().unwrap_or(false),
            west: buttons.get(2).copied().unwrap_or(false),
            north: buttons.get(3).copied().unwrap_or(false),
            left_shoulder: buttons.get(4).copied().unwrap_or(false),
            right_shoulder: buttons.get(5).copied().unwrap_or(false),
            left_trigger: *axes.first().unwrap_or(&0.0),
            right_trigger: *axes.get(1).unwrap_or(&0.0),
            select: buttons.get(8).copied().unwrap_or(false),
            start: buttons.get(9).copied().unwrap_or(false),
            guide: buttons.get(10).copied().unwrap_or(false),
            left_stick_button: buttons.get(6).copied().unwrap_or(false),
            right_stick_button: buttons.get(7).copied().unwrap_or(false),
            dpad_up,
            dpad_down,
            dpad_left,
            dpad_right,
            left_stick: [*axes.get(2).unwrap_or(&0.0), *axes.get(3).unwrap_or(&0.0)],
            right_stick: [0.0, 0.0],
        };

        let raw = RawGamepadState {
            device_id: dev_id,
            name: format!(
                "HID {}",
                match dev.get_product_string() {
                    Ok(Some(s)) => s,
                    _ => "Unknown".to_string(),
                }
            ),
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
            timestamp_ns: now_ns,
        };

        Some((canonical, raw))
    }
}

impl InputBackend for HidApiBackend {
    fn kind(&self) -> InputBackendKind {
        InputBackendKind::HidApi
    }

    fn poll(&mut self, now_ns: u64, sink: &mut dyn crate::backend::InputEventSink) {
        let Some(ref api) = self.api else { return };

        // Enumerate gamepad / joystick devices
        for dev_info in api.device_list() {
            let usage_page = dev_info.usage_page();
            let usage = dev_info.usage();

            // Only gamepad (0x05) or joystick (0x04) on Generic Desktop (0x01)
            if usage_page != 0x01 || (usage != 0x04 && usage != 0x05) {
                continue;
            }

            let path = dev_info.path().to_string_lossy().to_string();
            let dev_id = *self.device_map.entry(path.clone()).or_insert_with(|| {
                let id = PhysicalDeviceId {
                    backend: InputBackendKind::HidApi,
                    local_id: self.next_local_id,
                };
                self.next_local_id += 1;
                id
            });

            // Open device and poll
            if let Ok(dev) = dev_info.open_device(api) {
                if let Some((canonical, raw)) = self.poll_device(&path, &dev, dev_id, now_ns) {
                    let changed = self
                        .last_states
                        .get(&path)
                        .map_or(true, |last| *last != canonical);

                    if changed {
                        self.last_states.insert(path.clone(), canonical);
                        sink.on_raw_gamepad(raw);
                    }
                }
            }
        }
    }

    fn connected_devices(&self) -> Vec<InputDeviceInfo> {
        let Some(ref api) = self.api else {
            return Vec::new();
        };

        api.device_list()
            .filter(|d| d.usage_page() == 0x01 && (d.usage() == 0x04 || d.usage() == 0x05))
            .map(|d| {
                let path = d.path().to_string_lossy().to_string();
                let local_id = self
                    .device_map
                    .get(&path)
                    .map(|id| id.local_id)
                    .unwrap_or(0);
                InputDeviceInfo {
                    device_id: PhysicalDeviceId {
                        backend: InputBackendKind::HidApi,
                        local_id,
                    },
                    name: d.product_string().unwrap_or("HID Gamepad").to_string(),
                    vendor_id: Some(d.vendor_id()),
                    product_id: Some(d.product_id()),
                    backend: InputBackendKind::HidApi,
                }
            })
            .collect()
    }

    fn set_rumble(&mut self, _device: PhysicalDeviceId, _low: f32, _high: f32) -> Result<(), ()> {
        Err(()) // HID rumble is device-specific and not implemented here
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hid_backend_creation() {
        // HID API may or may not be available
        let backend = HidApiBackend::new();
        if let Some(b) = backend {
            assert_eq!(b.kind(), InputBackendKind::HidApi);
        }
    }
}
