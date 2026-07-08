# Legal and Licensing Information

## Project License

The NES Porting Toolkit is licensed under either of:

- **MIT License** ([http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
- **Apache License, Version 2.0** ([http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.

This dual-licensing is indicated in `Cargo.toml`:

```toml
[workspace.package]
license = "MIT OR Apache-2.0"
```

All source files in this repository are covered by this license unless
otherwise noted.

## ROM Distribution Policy

**This project does not distribute commercial ROMs, CHR dumps, original audio,
or other copyrighted game assets.** The repository contains:

- Source code for the toolkit, compilers, and runtime
- Game profile metadata (TOML/JSON configurations)
- Symbol tables, RAM maps, and hook annotations
- Test definitions and input replay files
- Documentation

Users must supply their own legally obtained ROM files. The toolkit reads these
files at runtime for analysis, recompilation, and execution.

### How to Obtain Legal ROMs

1. **Dump your own cartridge** using a ROM dumper (e.g., INLretro, Retrode,
   CopyNES, or a DIY Arduino-based reader).
2. **Purchase from authorised distributors** that sell ROMs with a license
   (e.g., some indie developers sell ROMs for their NES homebrew games).
3. **Homebrew and public domain ROMs** -- many NES homebrew games and demos
   are freely and legally available. The toolkit works with any valid iNES
   or NES 2.0 ROM, including homebrew and test ROMs.

This project does not condone or encourage software piracy. Use only ROMs
you have the legal right to use.

## Third-Party Dependency Licenses

The toolkit depends on several Rust crates. Below is a summary of key
dependencies and their licenses. For a complete and up-to-date list, run
`cargo license` or review `Cargo.lock`.

| Crate | License | Usage |
|---|---|---|
| `wgpu` | MIT OR Apache-2.0 | GPU rendering (Vulkan, Metal, D3D12, OpenGL, WebGPU) |
| `winit` | MIT OR Apache-2.0 | Cross-platform window creation and event loop |
| `egui` / `egui-wgpu` / `egui-winit` | MIT OR Apache-2.0 | Debug UI overlay |
| `disasm6502` | MIT | 6502 disassembler for static analysis |
| `cpal` | MIT OR Apache-2.0 | Low-level cross-platform audio output |
| `kira` | MIT OR Apache-2.0 | High-level game audio (mixer, spatial, etc.) |
| `mos6502` | MIT OR Apache-2.0 | 6502 CPU reference (optional) |
| `disasm6502` | MIT | 6502 disassembler |
| `serde` / `serde_json` | MIT OR Apache-2.0 | Serialization framework |
| `toml` | MIT OR Apache-2.0 | TOML configuration parsing |
| `ron` | MIT OR Apache-2.0 | RON config parsing (symbols, hooks, input) |
| `clap` | MIT OR Apache-2.0 | CLI argument parsing |
| `tracing` / `tracing-subscriber` | MIT | Structured logging |
| `bytemuck` | Apache-2.0 OR MIT OR Zlib | GPU POD type casting |
| `image` | MIT OR Apache-2.0 | CHR tile atlas PNG export |
| `gilrs` | MIT OR Apache-2.0 | Cross-platform gamepad input |
| `rusty-xinput` | MIT OR Apache-2.0 | Windows XInput support |
| `hidapi` | MIT OR Apache-2.0 | Raw HID device access |

This list is provided for convenience and does not constitute legal advice.
Review each dependency's license terms before redistribution.

A full license report can be generated with:

```bash
cargo install cargo-license
cargo license
```

## NESdev Wiki Attribution

The implementation of the NES CPU, PPU, APU, mapper, and ROM parsing
components in `nes-core` draws heavily on the technical documentation
published on the **NESdev Wiki** ([https://www.nesdev.org/](https://www.nesdev.org/)).

The NESdev Wiki is an invaluable community resource that documents the NES
hardware in extensive detail. Key pages referenced during development include:

- CPU memory map and 6502 instruction behaviour
- PPU registers, rendering pipeline, and timing
- APU frame counter and channel behaviour
- iNES and NES 2.0 ROM format specifications
- Mapper documentation

The NESdev Wiki content is available under the
[Creative Commons Attribution-ShareAlike 4.0 International](https://creativecommons.org/licenses/by-sa/4.0/) license unless otherwise noted.
This project's use of that information is in the form of independent Rust
implementations based on the documented hardware behaviour, and does not
include verbatim copies of wiki content.

## Data Crystal Attribution

The Battle City RAM map and symbol table used in the default GameProfile
(`profiles/battle_city/`) are based on reverse-engineering data published on
the **Data Crystal Wiki** ([https://datacrystal.tcrf.net/](https://datacrystal.tcrf.net/)).

Data Crystal is a community-driven ROM hacking and game data wiki. The Battle
City page provides RAM addresses for game state variables (lives, stage
counter, player position, etc.) that were used to construct the initial symbol
table.

Data Crystal content is generally available under the
[GNU Free Documentation License 1.2](https://www.gnu.org/licenses/old-licenses/fdl-1.2.html)
or compatible terms. As with the NESdev Wiki, this project uses the documented
addresses and semantics to build independent implementations rather than
including verbatim copies of wiki pages.

## Trademarks

- **NES** and **Famicom** are trademarks of Nintendo Co., Ltd. This project
  is not affiliated with, endorsed by, or sponsored by Nintendo.
- **Battle City** is a trademark of Namco Ltd. (now Bandai Namco Entertainment).
  This project is not affiliated with or endorsed by Bandai Namco.
- All other trademarks are the property of their respective owners.

## Contributions

By contributing to this project, you agree that your contributions will be
licensed under the same terms as the project (MIT OR Apache-2.0), and that
you have the right to grant this license for your contributions.

## Disclaimer

THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
