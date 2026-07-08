# Input System Backend Documentation

## Architecture

The input system (`nes-input` crate) translates physical input devices
(keyboards, gamepads) into canonical gamepad state, and then
maps that canonical state into NES controller button readings.

```
Physical Device (keyboard / gamepad)
    |
    v
Platform Backend (InputBackend trait)  <- polls hardware each frame
    |
    v
RawGamepadState  (backend-agnostic: buttons vector + axes vector)
    |
    v
InputMapper  (MappingProfile: maps named buttons/axes to canonical fields)
    |
    v
CanonicalGamepadState  (backed-agnostic: named fields like south, dpad_up, etc.)
    |
    v
canonical_to_nes_port()  (CanonicalGamepadState -> NesControllerState)
    |
    v
NesControllerPort (shift register emulation)
    |
    v
NES Bus ($4016 / $4017 reads)
```

## Backend Architecture Diagram

```
+------------------+
|   InputBackend   |  trait: poll(), kind(), connected_devices(), set_rumble()
+------------------+
        ^
        |  implements
+------------------+------------------+------------------+------------------+
|                  |                  |                  |                  |
| WinitKeyboard    | GilrsBackend     | HidApiBackend    | ReplayBackend    |
| (all platforms)  | (cross-platform) | (cross-platform) | (deterministic)  |
+------------------+------------------+------------------+------------------+
```

## InputBackend Trait

```rust
pub trait InputBackend {
    fn kind(&self) -> InputBackendKind;
    fn poll(&mut self, now_ns: u64, sink: &mut dyn InputEventSink);
    fn connected_devices(&self) -> Vec<InputDeviceInfo>;
    fn set_rumble(&mut self, device: PhysicalDeviceId, low: f32, high: f32) -> Result<(), ()>;
}
```

| Method | Description |
|---|---|
| `kind()` | Returns the backend type identifier |
| `poll()` | Scans for new input data and delivers events to the sink |
| `connected_devices()` | Lists all currently connected physical devices |
| `set_rumble()` | Activates force-feedback on a device |

Input backends deliver events through the `InputEventSink` trait:

```rust
pub trait InputEventSink {
    fn on_raw_gamepad(&mut self, state: RawGamepadState);
    fn on_device_connected(&mut self, info: InputDeviceInfo);
    fn on_device_disconnected(&mut self, id: PhysicalDeviceId);
}
```

### Backend Kind Identifiers

```rust
pub enum InputBackendKind {
    WinitKeyboard,
    Gilrs,
    HidApi,
    Replay,
}
```

## CanonicalGamepadState Field Reference

`CanonicalGamepadState` is the backend-agnostic representation of a gamepad:

```rust
pub struct CanonicalGamepadState {
    // Face buttons (SNES / Xbox layout)
    pub south: bool,          // Xbox A / SNES B / PS Cross
    pub east: bool,           // Xbox B / SNES A / PS Circle
    pub west: bool,           // Xbox X / SNES Y / PS Square
    pub north: bool,          // Xbox Y / SNES X / PS Triangle

    // Shoulder buttons
    pub left_shoulder: bool,  // LB / L1
    pub right_shoulder: bool, // RB / R1

    // Analog triggers (0.0 to 1.0)
    pub left_trigger: f32,    // LT / L2
    pub right_trigger: f32,   // RT / R2

    // Menu / special
    pub select: bool,         // Back / Share / Select
    pub start: bool,          // Start / Options / Menu
    pub guide: bool,          // Xbox button / PS button

    // Stick clicks
    pub left_stick_button: bool,   // L3
    pub right_stick_button: bool,  // R3

    // D-pad
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,

    // Analog sticks (X, Y) in range [-1.0, 1.0]
    pub left_stick: [f32; 2],
    pub right_stick: [f32; 2],
}
```

## NES Controller Mapping

The `canonical_to_nes_port()` function maps `CanonicalGamepadState` to
`NesControllerState` (8 discrete buttons):

### Port 1 (Primary Player)

| NES Button | Canonical Field |
|---|---|
| A | `south` (Xbox A / PS Cross) |
| B | `east` (Xbox B / PS Circle) |
| Select | `select` |
| Start | `start` |
| Up | `dpad_up` |
| Down | `dpad_down` |
| Left | `dpad_left` |
| Right | `dpad_right` |

### Port 2 (Secondary Player / Shared Gamepad Mode)

| NES Button | Canonical Field |
|---|---|
| A | `west` (Xbox X / PS Square) |
| B | `north` (Xbox Y / PS Triangle) |
| Select | `right_stick_button` |
| Start | `guide` |
| Up | `dpad_up` |
| Down | `dpad_down` |
| Left | `dpad_left` |
| Right | `dpad_right` |

### NesControllerState

```rust
pub struct NesControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}
```

**Shift register encoding** (bit order when reading `$4016`/`$4017`):
Bit 0 = A, Bit 1 = B, Bit 2 = Select, Bit 3 = Start, Bit 4 = Up, Bit 5 = Down,
Bit 6 = Left, Bit 7 = Right.

**Opposite direction policy:** When configured as `"neutralize"`, if both
Left+Right or both Up+Down are simultaneously asserted, both are cleared to
prevent undefined behaviour in games that do not expect opposing inputs.

## Input Replay Format

The replay system records and plays back deterministic input sequences for
testing and TAS purposes.

```rust
pub struct InputReplay {
    pub version: u8,                      // Format version (currently 1)
    pub fps: u8,                          // Frames per second (typically 60)
    pub frames: Vec<InputReplayFrame>,    // Frame-by-frame input data
}

pub struct InputReplayFrame {
    pub frame: u64,                       // Absolute frame number
    pub port1: Vec<String>,               // Active button names on port 1
    pub port2: Option<Vec<String>>,       // Active button names on port 2
}
```

### Button Name Reference

| Name | Meaning |
|---|---|
| `"A"` | NES A button |
| `"B"` | NES B button |
| `"SELECT"` | Select button |
| `"START"` | Start button |
| `"UP"` | D-pad up |
| `"DOWN"` | D-pad down |
| `"LEFT"` | D-pad left |
| `"RIGHT"` | D-pad right |

### Replay Example (JSON serialisation)

```json
{
    "version": 1,
    "fps": 60,
    "frames": [
        {"frame": 0, "port1": ["A", "START"]},
        {"frame": 2, "port1": ["B", "UP"], "port2": ["LEFT"]},
        {"frame": 120, "port1": []}
    ]
}
```

**Hold behaviour:** If a frame has no explicit entry, the last frame with
`frame <= current_frame` is used. This avoids repeating identical input data
for every frame.

**ReplayBackend API:**

```rust
let mut backend = ReplayBackend::new(replay);

// Get state for a specific frame
let state = backend.state_for_frame(42, 1);

// Advance frame counter and return new state
let next = backend.advance_frame();

// Reset to frame 0
backend.reset();
```

## How to Add a New Backend

1. **Create a new module** under `crates/nes-input/src/backends/`.
   Name it after the API you are wrapping (e.g. `sdl_gamepad.rs`).

2. **Implement `InputBackend`:**
   ```rust
   use crate::backend::{InputBackend, InputBackendKind, InputDeviceInfo,
                         PhysicalDeviceId, RawGamepadState, InputEventSink};

   pub struct MyBackend {
       // platform-specific state (device handles, event queues, etc.)
   }

   impl MyBackend {
       pub fn new() -> Self { /* ... */ }
   }

   impl InputBackend for MyBackend {
       fn kind(&self) -> InputBackendKind {
           InputBackendKind::HidApi  // or add a new variant
       }

       fn poll(&mut self, now_ns: u64, sink: &mut dyn InputEventSink) {
           // 1. Read current device state from the platform API
           // 2. For each connected device, build a RawGamepadState:
           //    - device_id: unique PhysicalDeviceId
           //    - buttons: Vec<bool> for each button
           //    - axes: Vec<f32> for each axis (normalised to [-1.0, 1.0])
           //    - timestamp_ns: monotonic timestamp
           // 3. Call sink.on_raw_gamepad(state) for each device
           // 4. Handle connect/disconnect events via sink callbacks
       }

       fn connected_devices(&self) -> Vec<InputDeviceInfo> {
           // Enumerate currently connected devices
           vec![]
       }

       fn set_rumble(&mut self, device: PhysicalDeviceId,
                     low: f32, high: f32) -> Result<(), ()> {
           // Activate rumble motors (optional, return Err(()) if unsupported)
           Err(())
       }
   }
   ```

3. **Register a new `InputBackendKind` variant** in `backend.rs` if your
   backend is not covered by an existing variant.

4. **Add a mapping profile** if your backend uses different button/axis
   naming conventions. The `InputMapper` maps backend-specific names
   (like `"BTN_0"`, `"ABS_X"`) to `CanonicalField` values.

5. **Update `backend_policy`** in the input profile RON to include your
   backend in the priority list for the relevant platform(s).

6. **Add tests:** Validate that your backend produces correct
   `CanonicalGamepadState` from known inputs, and that hotplug events
   are emitted correctly.

## Mouse and Keyboard Mapping

The `keyboard_to_canonical()` function in `backend.rs` maps key names
directly to `CanonicalGamepadState` fields:

| Key (case-insensitive) | Canonical Field |
|---|---|
| `W`, `ArrowUp` | `dpad_up` |
| `S`, `ArrowDown` | `dpad_down` |
| `A`, `ArrowLeft` | `dpad_left` |
| `D`, `ArrowRight` | `dpad_right` |
| `Z` | `south` (NES A) |
| `X` | `east` (NES B) |
| `Enter`, `Space` | `start` |
| `RShift` | `select` |
| `Q` | `left_shoulder` |
| `E` | `right_shoulder` |
| `Tab` | `guide` |
| `LControl` | `left_stick_button` |
| `RControl` | `right_stick_button` |
| `N` | `west` |
| `M` | `north` |

## Hotplug

The `HotplugManager` debounces device connection events and tracks
currently connected devices:

```rust
let mut hotplug = HotplugManager::new();

// Returns true only for genuinely new devices
if hotplug.handle_event(&device_info) {
    println!("New device connected: {}", device_info.name);
}

// Tracks disconnections
hotplug.handle_disconnect(&device_id);

// Query state
println!("{} devices connected", hotplug.count());
```

## See Also

- [PROFILE_FORMAT.md](PROFILE_FORMAT.md) -- input profile RON format and `[input]` section
- [RUNTIME_ABI.md](RUNTIME_ABI.md) -- how the runtime exposes controller state
- [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) -- overall project status
