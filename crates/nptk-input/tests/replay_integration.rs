//! Integration tests for nptk-input — replay roundtrip and mapping.

use nptk_input::backend::{InputBackendKind, InputDeviceInfo, PhysicalDeviceId};
use nptk_input::canonical::CanonicalGamepadState;
use nptk_input::replay::{InputReplay, InputReplayFrame, ReplayBackend};

// ── Input Replay roundtrip ──────────────────────────────────────────────

#[test]
fn test_replay_roundtrip() {
    let frames = vec![
        InputReplayFrame {
            frame: 0,
            port1: vec!["A".into(), "RIGHT".into()],
            port2: None,
        },
        InputReplayFrame {
            frame: 1,
            port1: vec![], // all released
            port2: None,
        },
    ];
    let replay = InputReplay {
        version: 1,
        fps: 60,
        frames,
    };
    let backend = ReplayBackend::new(replay);

    // Frame 0: A + Right pressed
    let s0 = backend.state_for_frame(0, 1);
    assert!(s0.a);
    assert!(s0.right);
    assert!(!s0.b);

    // Frame 1: all released
    let s1 = backend.state_for_frame(1, 1);
    assert!(!s1.a);
    assert!(!s1.right);

    // Frame 3 (no data — hold previous): should be frame 1's state
    let s3 = backend.state_for_frame(3, 1);
    assert!(!s3.a);
}

#[test]
fn test_replay_advance_reset() {
    let frames = vec![InputReplayFrame {
        frame: 0,
        port1: vec!["START".into()],
        port2: None,
    }];
    let replay = InputReplay {
        version: 1,
        fps: 60,
        frames,
    };
    let mut backend = ReplayBackend::new(replay);

    // advance_frame advances frame counter, returns state
    let _s = backend.advance_frame();
    // After advancing past frame 0, state holds frame 0 values
    // (advance_frame internally calls state_for_frame with the new frame)

    // reset brings back to frame 0
    backend.reset();
}

// ── Canonical → NES mapping ────────────────────────────────────────────

#[test]
fn test_canonical_to_nes_port1() {
    let canonical = CanonicalGamepadState {
        south: true,
        east: true,
        ..Default::default()
    };
    let nes = nptk_input::nes_controller::canonical_to_nes_port(&canonical, 1);
    assert!(nes.a); // south → A
    assert!(nes.b); // east → B
    assert!(!nes.start);
    assert!(!nes.select);
}

#[test]
fn test_canonical_to_nes_port2() {
    let canonical = CanonicalGamepadState {
        west: true,
        north: true,
        guide: true,
        right_stick_button: true,
        ..Default::default()
    };
    let nes = nptk_input::nes_controller::canonical_to_nes_port(&canonical, 2);
    assert!(nes.a); // west → A (port 2)
    assert!(nes.b); // north → B (port 2)
    assert!(nes.start); // guide → Start (port 2)
    assert!(nes.select); // right_stick_button → Select (port 2)
}

// ── Hotplug manager ─────────────────────────────────────────────────────

#[test]
fn test_hotplug_connect_disconnect() {
    use nptk_input::hotplug::HotplugManager;

    let mut mgr = HotplugManager::new();
    let dev_id = PhysicalDeviceId {
        backend: InputBackendKind::Gilrs,
        local_id: 0,
    };

    let dev_info = InputDeviceInfo {
        device_id: dev_id,
        name: "Test Gamepad".into(),
        vendor_id: Some(0x045E),
        product_id: Some(0x028E),
        backend: InputBackendKind::Gilrs,
    };

    // Connect
    assert!(mgr.handle_event(&dev_info));
    assert!(mgr.is_connected(&dev_info.device_id));
    assert_eq!(mgr.count(), 1);

    // Duplicate connect is a no-op (already tracked)
    assert!(!mgr.handle_event(&dev_info));
    assert_eq!(mgr.count(), 1);

    // Disconnect
    assert!(mgr.handle_disconnect(&dev_info.device_id));
    assert!(!mgr.is_connected(&dev_info.device_id));
    assert_eq!(mgr.count(), 0);
}

// ── Backend kind existence ─────────────────────────────────────────────

#[test]
fn test_input_backend_kinds_exist() {
    // Verify all expected backend kinds are defined
    let gilrs = InputBackendKind::Gilrs;
    let keyboard = InputBackendKind::WinitKeyboard;
    let hidapi = InputBackendKind::HidApi;
    let replay = InputBackendKind::Replay;
    // Just verify they can be constructed
    let _ = (gilrs, keyboard, hidapi, replay);
}
