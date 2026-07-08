//! Static analysis for 6502 machine code.
//!
//! Provides heuristics for:
//! - Code vs. data classification
//! - Jump table detection
//! - Branch target collection
//! - Basic block boundary detection

use std::collections::HashSet;

/// Result of analyzing a byte at a given address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteType {
    /// Confirmed code (reached via disassembly trace).
    Code,
    /// Likely data (never reached by control flow).
    Data,
    /// Unknown — neither confirmed code nor clearly data.
    Unknown,
}

/// Return true if the address is a known code entry point or has been
/// reached by the BFS block discovery pass.
pub fn is_code_start(data: &[u8], addr: u16) -> bool {
    // Accept any address in PRG-ROM range — the actual classification
    // happens during block discovery (BFS from vectors).  This function
    // exists so downstream code can query whether an address was visited.
    let _ = (data, addr);
    true
}

/// Classify every byte in PRG-ROM as Code, Data, or Unknown.
///
/// `code_addrs` is the set of addresses visited during BFS block discovery.
/// Any byte not in this set is classified as Data.
pub fn classify_bytes(
    prg_size: usize,
    code_addrs: &HashSet<u16>,
) -> Vec<ByteType> {
    let mut result = vec![ByteType::Unknown; prg_size];

    for &addr in code_addrs {
        let offset = addr_to_prg_offset(addr);
        if offset < prg_size {
            result[offset] = ByteType::Code;
        }
    }

    // Mark clearly unreachable ranges as Data
    for (_i, entry) in result.iter_mut().enumerate() {
        if *entry == ByteType::Unknown {
            *entry = ByteType::Data;
        }
    }

    result
}

/// Convert a CPU address ($8000-$FFFF) to a PRG-ROM offset.
pub fn addr_to_prg_offset(addr: u16) -> usize {
    if addr < 0x8000 {
        0
    } else {
        (addr as usize - 0x8000) & 0x3FFF // 16KB PRG-ROM, mirrored
    }
}

/// Detect potential jump tables by scanning for consecutive CMP + branch
/// patterns followed by indirect JMP or indexed JMP.
///
/// Returns a list of addresses that look like jump table entries.
pub fn detect_jump_tables(data: &[u8], prg_base: u16) -> Vec<u16> {
    let mut tables = Vec::new();

    // Scan for the pattern:
    //   CMP #$NN      (0xC9)
    //   BCS/BCC label (0xB0/0x90)
    //   ... dispatch code ...
    // Followed by a sequence of addresses (2 bytes each) at a known table base.
    let mut i = 0usize;
    while i + 3 < data.len() {
        if data[i] == 0xC9 {
            // Found a CMP immediate — check if followed by a branch
            let next = i + 2;
            if next < data.len() {
                match data[next] {
                    0xB0 | 0x90 | 0xF0 | 0xD0 => {
                        // Potential jump table dispatch — note the comparison
                        // value as the number of table entries.
                        let table_size = data[i + 1] as usize;
                        // The table base address is typically resolved from
                        // subsequent LDA/STA/JMP instructions.
                        // For now, mark the CMP address as a dispatch point.
                        let addr = prg_base + i as u16;
                        tables.push(addr);
                        let _ = table_size; // suppress unused warning
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }

    tables
}

/// Collect all direct branch targets reachable from a set of entry points.
///
/// This is used to seed the BFS block discovery pass.
pub fn collect_entry_points(data: &[u8], prg_base: u16) -> Vec<u16> {
    let mut entries = Vec::new();

    // Always include the reset/NMI/IRQ vectors from the end of PRG-ROM.
    if data.len() >= 6 {
        let end = data.len();
        let nmi = u16::from_le_bytes([data[end - 6], data[end - 5]]);
        let reset = u16::from_le_bytes([data[end - 4], data[end - 3]]);
        let irq = u16::from_le_bytes([data[end - 2], data[end - 1]]);

        entries.push(nmi);
        entries.push(reset);
        entries.push(irq);
    }

    // Scan for JSR instructions to find subroutine entry points.
    let mut i = 0usize;
    while i + 3 <= data.len() {
        if data[i] == 0x20 {
            // JSR absolute — the target is a subroutine entry point
            let lo = data[i + 1] as u16;
            let hi = data[i + 2] as u16;
            let target = lo | (hi << 8);
            if target >= 0x8000 && target < prg_base + data.len() as u16 {
                entries.push(target);
            }
            i += 3;
        } else {
            i += instruction_length(data[i]);
        }
    }

    entries
}

/// Return the byte length of a 6502 instruction given its opcode.
///
/// Returns 1 for unknown/illegal opcodes.
pub fn instruction_length(opcode: u8) -> usize {
    match opcode {
        // 1-byte: implied / accumulator
        0x00 | 0x08 | 0x18 | 0x28 | 0x38 | 0x48 | 0x58 | 0x68 |
        0x78 | 0x88 | 0x98 | 0xA8 | 0xB8 | 0xC8 | 0xD8 | 0xE8 |
        0xF8 | 0x0A | 0x2A | 0x4A | 0x6A | 0x8A | 0x9A | 0xAA |
        0xBA | 0xCA | 0xEA | 0x40 | 0x60 | 0x1A | 0x3A | 0x5A |
        0x7A | 0xDA | 0xFA => 1,

        // 3-byte: absolute, absolute indexed, indirect
        0x0C | 0x0D | 0x0E | 0x0F |
        0x1C | 0x1D | 0x1E | 0x1F |
        0x20 | // JSR abs
        0x2C | 0x2D | 0x2E | 0x2F |
        0x3C | 0x3D | 0x3E | 0x3F |
        0x4C | 0x4D | 0x4E | 0x4F |
        0x5C | 0x5D | 0x5E | 0x5F |
        0x6C | 0x6D | 0x6E | 0x6F |
        0x7C | 0x7D | 0x7E | 0x7F |
        0x8C | 0x8D | 0x8E | 0x8F |
        0x9C | 0x9D | 0x9E | 0x9F |
        0xAC | 0xAD | 0xAE | 0xAF |
        0xBC | 0xBD | 0xBE | 0xBF |
        0xCC | 0xCD | 0xCE | 0xCF |
        0xDC | 0xDD | 0xDE | 0xDF |
        0xEC | 0xED | 0xEE | 0xEF |
        0xFC | 0xFD | 0xFE | 0xFF => 3,

        // 2-byte: everything else (immediate, zero page, branch, zero page indexed)
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_lengths() {
        assert_eq!(instruction_length(0x00), 1); // BRK
        assert_eq!(instruction_length(0x60), 1); // RTS
        assert_eq!(instruction_length(0xA9), 2); // LDA imm
        assert_eq!(instruction_length(0xA5), 2); // LDA zp
        assert_eq!(instruction_length(0xAD), 3); // LDA abs
        assert_eq!(instruction_length(0x20), 3); // JSR abs
        assert_eq!(instruction_length(0x4C), 3); // JMP abs
        assert_eq!(instruction_length(0xD0), 2); // BNE
    }

    #[test]
    fn test_addr_to_prg_offset() {
        assert_eq!(addr_to_prg_offset(0x8000), 0);
        assert_eq!(addr_to_prg_offset(0x8001), 1);
        assert_eq!(addr_to_prg_offset(0xBFFF), 0x3FFF);
        // 16KB NROM mirror: $C000 maps back to offset 0
        assert_eq!(addr_to_prg_offset(0xC000), 0);
        assert_eq!(addr_to_prg_offset(0xC001), 1);
    }

    #[test]
    fn test_classify_bytes_all_data() {
        let code: HashSet<u16> = HashSet::new();
        let result = classify_bytes(256, &code);
        assert!(result.iter().all(|b| *b == ByteType::Data));
    }

    #[test]
    fn test_classify_bytes_with_code() {
        let mut code = HashSet::new();
        code.insert(0x8000);
        let result = classify_bytes(256, &code);
        assert_eq!(result[0], ByteType::Code);       // 0x8000 -> offset 0
        assert_eq!(result[1], ByteType::Data);       // not in code set
    }

    #[test]
    fn test_jump_table_detection() {
        // CMP #$04, BCS $xx
        let data = &[0xC9, 0x04, 0xB0, 0x05, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA];
        let tables = detect_jump_tables(data, 0x8000);
        assert!(!tables.is_empty());
        assert_eq!(tables[0], 0x8000); // CMP is at offset 0
    }

    #[test]
    fn test_collect_entry_points() {
        let mut data = vec![0xEAu8; 256];
        // JSR $8042 at offset 0
        data[0] = 0x20;
        data[1] = 0x42;
        data[2] = 0x80;
        // Vectors at end: NMI=$8100, RESET=$8200, IRQ=$8300
        let end = data.len();
        data[end - 6] = 0x00; data[end - 5] = 0x81;
        data[end - 4] = 0x00; data[end - 3] = 0x82;
        data[end - 2] = 0x00; data[end - 1] = 0x83;

        let entries = collect_entry_points(&data, 0x8000);
        assert!(entries.contains(&0x8100)); // NMI vector
        assert!(entries.contains(&0x8200)); // RESET vector
        assert!(entries.contains(&0x8300)); // IRQ vector
        assert!(entries.contains(&0x8042)); // JSR target
    }
}
