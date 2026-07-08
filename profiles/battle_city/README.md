# Battle City Profile

**Game:** Battle City (Namco, 1985)  
**Platform:** Nintendo Entertainment System / Famicom  
**Mapper:** 0 (NROM)  
**PRG-ROM:** 16 KiB  
**CHR-ROM:** 8 KiB  
**Mirroring:** Horizontal  
**SRAM:** None

## Profile Contents

| File | Purpose |
|---|---|
| `profile.toml` | Main game profile — ROM metadata, CPU/PPU/audio/input configuration |
| `symbols.ron` | Symbol table — named addresses for RAM, functions, and data sections |
| `ram_map.ron` | Annotated RAM map with descriptions for key memory locations |
| `hooks.ron` | Hooks for static recompiler — function entry points and data tables |
| `input.ron` | Input configuration — port mapping, bindings, backend policy |
| `tests.ron` | Automated test definitions for trace comparison and golden frames |

## Expected ROM

The profile expects a Battle City (Japan) ROM dump with:

- **PRG CRC32:** `2b1b2cb2`
- **CHR CRC32:** `ae8b1f50`

CRC values are for identification only. The repository does not distribute ROM content.

## Status

This profile is the default implementation target for the NES Porting Toolkit.
It is actively developed and tested. See `docs/plan.md` for the implementation
roadmap.