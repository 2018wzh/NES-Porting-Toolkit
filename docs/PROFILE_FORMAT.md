# GameProfile Format Reference

A **GameProfile** describes everything the toolkit needs to know about a
specific NES/Famicom game: ROM identification, CPU behaviour, PPU/audio/input
policies, testing configuration, and symbol tables. Profiles are written in
TOML with companion RON files for structured data (symbols, hooks, input
mappings, RAM maps, test definitions).

## Directory Layout

```
profiles/<game_id>/
  profile.toml    -- main GameProfile (TOML)
  symbols.ron     -- symbol table (RON)
  ram_map.ron     -- commented RAM address map (RON)
  hooks.ron       -- code region annotations (RON)
  input.ron       -- input port mappings (RON)
  tests.ron       -- automated test cases (RON)
  README.md       -- profile description
```

## TOML Profile Structure

### `[game]` Section

| Field | Type | Description |
|---|---|---|
| `id` | `String` | Unique internal identifier (e.g. `"battle_city"`) |
| `display_name` | `String` | Human-readable game name |
| `region` | `Option<String>` | Release region: `"JP"`, `"US"`, `"EU"`, etc. |
| `default_mode` | `String` | Default run mode: `"compat-interpreter"`, `"recompiled-compat"`, `"native-port"`, or `"auto"` |

### `[rom]` Section

| Field | Type | Description |
|---|---|---|
| `system` | `String` | Always `"nes"` |
| `accepted_format` | `Vec<String>` | Accepted ROM formats: `["ines"]`, `["nes20"]`, or both |
| `mapper` | `u16` | iNES mapper number (e.g. `0` for NROM) |
| `mapper_name` | `Option<String>` | Human-readable mapper name (e.g. `"NROM"`) |
| `mirroring` | `String` | Nametable mirroring: `"horizontal"`, `"vertical"`, `"four-screen"` |
| `prg_size` | `usize` | Expected PRG-ROM size in bytes |
| `chr_size` | `usize` | Expected CHR-ROM size in bytes |
| `has_sram` | `bool` | Whether the cartridge has battery-backed SRAM |

#### `[[rom.known_dump]]` Array

Each entry describes a known ROM dump for identification:

| Field | Type | Description |
|---|---|---|
| `name` | `String` | Dump name (e.g. `"Battle City (J)"`) |
| `prg_crc32` | `Option<String>` | CRC-32 of PRG-ROM data |
| `chr_crc32` | `Option<String>` | CRC-32 of CHR-ROM data |
| `combined_crc32` | `Option<String>` | CRC-32 of combined ROM |

### `[cpu]` Section

| Field | Type | Description |
|---|---|---|
| `reset_vector` | `Option<String>` | Reset vector address (`"auto"` or hex like `"0x8000"`) |
| `nmi_vector` | `Option<String>` | NMI vector address (`"auto"` or hex) |
| `irq_vector` | `Option<String>` | IRQ/BRK vector address (`"auto"` or hex) |
| `allow_decimal_mode` | `bool` | Whether decimal mode (SED/CLD) is permitted. Most NES games do not use decimal mode. |
| `unknown_indirect_jump` | `String` | Strategy for unresolved indirect jumps: `"dispatcher"` (generate jump table), `"trap"` (abort on hit) |

### `[ppu]` Section

| Field | Type | Description |
|---|---|---|
| `initial_mode` | `String` | Starting PPU mode: `"compat"` (software PPU) or `"native"` (WGPU) |
| `native_mode` | `String` | Native rendering strategy: `"tilemap_sprite"` |
| `sprite_source` | `String` | Sprite data source: `"oam"` |
| `background_source` | `String` | Background data source: `"nametable"` |
| `chr_export` | `bool` | Whether to export CHR tiles as PNG atlas |
| `palette_policy` | `String` | Palette handling: `"nes_palette"` (NES 64-colour palette) or `"exact"` |

### `[audio]` Section

| Field | Type | Description |
|---|---|---|
| `initial_mode` | `String` | Starting audio mode: `"apu_compat"` (software APU) or `"native"` |
| `native_sfx` | `String` | Native sound effect strategy: `"optional"`, `"none"`, or `"required"` |
| `native_bgm` | `String` | Native background music strategy: `"optional"`, `"none"`, or `"required"` |

### `[input]` Section

| Field | Type | Description |
|---|---|---|
| `controller_ports` | `u8` | Number of controller ports (1 or 2) |
| `default_backend_policy` | `String` | Backend selection policy: `"auto"` or path to custom RON |
| `default_port_1` | `String` | Default Port 1 device: `"keyboard_gamepad"`, `"nes_standard"` |
| `input_profile` | `Option<String>` | Path to a RON input mapping file |

### `[testing]` Section

| Field | Type | Description |
|---|---|---|
| `enable_trace_compare` | `bool` | Enable CPU trace comparison (interpreter vs. recompiled) |
| `enable_golden_frames` | `bool` | Enable frame hash golden testing |
| `enable_input_replay` | `bool` | Enable deterministic input replay |

## RON Symbol Table Format (`symbols.ron`)

The symbol table maps human-readable names to 16-bit NES addresses,
organised into three categories:

```ron
(
    ram: {
        "lives": 0x0051,
        "stage_counter": 0x0085,
        "game_mode": 0x0078,
    },
    functions: {
        "nmi_handler": 0xFFF0,
        "reset_handler": 0xFFFC,
        "irq_handler": 0xFFFE,
        "title_screen": 0xE000,
        "game_init": 0xE100,
        "player_move": 0xE200,
        "enemy_ai": 0xE300,
        "bullet_update": 0xE400,
        "collision_check": 0xE500,
        "stage_load": 0xE600,
    },
    data: {
        "stage_data": 0xC000,
        "tank_sprites": 0xD000,
        "bullet_sprites": 0xD800,
        "explosion_sprites": 0xD900,
        "level_layouts": 0xC800,
    },
)
```

**Fields:**

- `ram`: Zero-page and stack variables, keyed by name, valued by NES CPU address (`$0000-$07FF`)
- `functions`: Code entry points and subroutines, keyed by name, valued by PRG-ROM address (`$8000-$FFFF`)
- `data`: Data tables (stage layouts, sprite definitions, lookup tables), keyed by name, valued by PRG-ROM address

## RON Hooks Format (`hooks.ron`)

Hooks annotate code regions so the recompiler and native runtime know
how to treat each address range:

```ron
(
    hooks: [
        (address: 0xE000, name: "title_screen", hook_type: NamedFunction, size: Some(256), comment: Some("Title screen entry point")),
        (address: 0xE100, name: "game_init", hook_type: NamedFunction, size: Some(512), comment: Some("Game initialization")),
        (address: 0xE200, name: "player_move", hook_type: NamedFunction, size: Some(256), comment: Some("Player movement handler")),
        (address: 0xC000, name: "stage_data", hook_type: DataTable, size: Some(2048), comment: Some("Stage layout data")),
        (address: 0xC800, name: "level_layouts", hook_type: DataTable, size: Some(2048), comment: Some("Level layout pointer table")),
    ],
)
```

**Hook types:**

| `hook_type` | Meaning |
|---|---|
| `NamedFunction` | A callable subroutine entry point |
| `DataTable` | A read-only data table (skip code discovery) |
| `JumpTable` | An indirect jump dispatch table |
| `SkipRegion` | A region the recompiler should ignore |
| `HardwareRegister` | A memory-mapped I/O region |

Each hook specifies:

| Field | Type | Description |
|---|---|---|
| `address` | `u16` | Starting address in CPU address space |
| `name` | `String` | Human-readable label |
| `hook_type` | `HookType` | Classification (see table above) |
| `size` | `Option<u16>` | Size of the region in bytes |
| `comment` | `Option<String>` | Free-form annotation |

## RON Input Mapping Format (`input.ron`)

See [INPUT_BACKENDS.md](INPUT_BACKENDS.md) for the full input system
documentation. The input RON format used in profiles:

```ron
(
    ports: [
        (
            port: 1,
            sources: ["keyboard", "gamepad0"],
            mapping: {
                "nes_a":      ["keyboard:KeyZ", "gamepad:South"],
                "nes_b":      ["keyboard:KeyX", "gamepad:East"],
                "nes_start":  ["keyboard:Enter", "gamepad:Start"],
                "nes_select": ["keyboard:ShiftRight", "gamepad:Select"],
                "nes_up":     ["keyboard:ArrowUp", "gamepad:DPadUp"],
                "nes_down":   ["keyboard:ArrowDown", "gamepad:DPadDown"],
                "nes_left":   ["keyboard:ArrowLeft", "gamepad:DPadLeft"],
                "nes_right":  ["keyboard:ArrowRight", "gamepad:DPadRight"],
            },
            opposite_direction_policy: "neutralize",
            analog_deadzone: 0.25,
            analog_hysteresis: 0.05,
        ),
        (
            port: 2,
            sources: ["gamepad1"],
            mapping: "default_nes",
            opposite_direction_policy: "neutralize",
            analog_deadzone: 0.25,
            analog_hysteresis: 0.05,
        ),
    ],
    backend_policy: (
        windows: ["gilrs_wgi", "xinput", "keyboard"],
        linux:   ["gilrs_evdev", "keyboard"],
        macos:   ["gilrs", "keyboard"],
        wasm:    ["web_gamepad", "keyboard"],
    ),
)
```

## RAM Map Format (`ram_map.ron`)

An annotated version of the symbol table's RAM section, with inline comments:

```ron
(
    ram: {
        // $0051: Player lives (0 = game over)
        "lives": 0x0051,
        // $0085: Current stage number
        "stage_counter": 0x0085,
        // $0078: Game mode (0=title, 1=playing, 2=game over)
        "game_mode": 0x0078,
        // $00A6: Player X position (pixels)
        "player_x": 0x00A6,
        // $00A7: Player Y position (pixels)
        "player_y": 0x00A7,
    },
)
```

## Tests Format (`tests.ron`)

```ron
(
    tests: [
        (name: "title_screen", frames: 60, input: "none", expected_ram: {"0x0078": 0x00}),
        (name: "start_game", frames: 120, input: "start_button", expected_ram: {"0x0078": 0x01}),
    ],
)
```

| Field | Type | Description |
|---|---|---|
| `name` | `String` | Test case name |
| `frames` | `u64` | Number of frames to execute |
| `input` | `String` | Input source: `"none"`, `"start_button"`, or path to replay file |
| `expected_ram` | `BTreeMap<String, u8>` | Expected RAM values after execution, keyed by hex address string |

## Example: Battle City Profile Snippet

```toml
[game]
id = "battle_city"
display_name = "Battle City"
region = "JP"
default_mode = "compat-interpreter"

[rom]
system = "nes"
accepted_format = ["ines", "nes20"]
mapper = 0
mapper_name = "NROM"
mirroring = "horizontal"
prg_size = 16384
chr_size = 8192
has_sram = false

[[rom.known_dump]]
name = "Battle City (J)"
prg_crc32 = "2b1b2cb2"
chr_crc32 = "ae8b1f50"

[cpu]
reset_vector = "auto"
nmi_vector = "auto"
irq_vector = "auto"
allow_decimal_mode = false
unknown_indirect_jump = "dispatcher"

[ppu]
initial_mode = "compat"
native_mode = "tilemap_sprite"
sprite_source = "oam"
background_source = "nametable"
chr_export = true
palette_policy = "nes_palette"

[audio]
initial_mode = "apu_compat"
native_sfx = "optional"
native_bgm = "optional"

[input]
controller_ports = 2
default_backend_policy = "auto"
default_port_1 = "keyboard_gamepad"
input_profile = "profiles/battle_city/input.ron"

[testing]
enable_trace_compare = true
enable_golden_frames = true
enable_input_replay = true
```

## Loading Profiles in Code

```rust
use nes_profile::profile::load_profile;

let profile = load_profile("profiles/battle_city/profile.toml")?;
println!("Game: {}", profile.game.display_name);
println!("Mapper: {} ({})", profile.rom.mapper, profile.rom.mapper_name.unwrap_or_default());
```

## See Also

- [RUNTIME_ABI.md](RUNTIME_ABI.md) -- how recompiled code calls into the runtime
- [RECOMPILER.md](RECOMPILER.md) -- the static recompilation pipeline
- [INPUT_BACKENDS.md](INPUT_BACKENDS.md) -- input backend architecture
