//! NES controller state mapping from canonical gamepad to NES port.

pub use nptk_core::controller::NesControllerState;

/// Convert a `CanonicalGamepadState` into a `NesControllerState` for the given
/// player port (1 or 2).  Port 1 uses the conventional mapping; port 2 reads
/// from the right-stick / right-shoulder region so two players can share one
/// physical gamepad.
pub fn canonical_to_nes_port(
    state: &crate::canonical::CanonicalGamepadState,
    port: u8,
) -> NesControllerState {
    match port {
        1 => NesControllerState {
            a: state.south,
            b: state.east,
            start: state.start,
            select: state.select,
            up: state.dpad_up,
            down: state.dpad_down,
            left: state.dpad_left,
            right: state.dpad_right,
        },
        2 => NesControllerState {
            // port 2: face buttons shift to west/north
            a: state.west,
            b: state.north,
            start: state.guide,
            select: state.right_stick_button,
            up: state.dpad_up,
            down: state.dpad_down,
            left: state.dpad_left,
            right: state.dpad_right,
        },
        _ => NesControllerState::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::CanonicalGamepadState;

    #[test]
    fn port_1_maps_south_to_a() {
        let mut c = CanonicalGamepadState::default();
        c.south = true;
        c.start = true;
        let nes = canonical_to_nes_port(&c, 1);
        assert!(nes.a);
        assert!(!nes.b);
        assert!(nes.start);
    }

    #[test]
    fn port_2_maps_west_to_a() {
        let mut c = CanonicalGamepadState::default();
        c.west = true;
        c.guide = true;
        let nes = canonical_to_nes_port(&c, 2);
        assert!(nes.a);
        assert!(nes.start);
    }

    #[test]
    fn unknown_port_defaults() {
        let c = CanonicalGamepadState::default();
        let nes = canonical_to_nes_port(&c, 99);
        assert_eq!(nes, NesControllerState::default());
    }
}
