//! iNES / NES 2.0 ROM parser
//!
//! 参考 NESDev Wiki: https://www.nesdev.org/wiki/NES_2.0

use super::types::*;

const INES_SIGNATURE: [u8; 4] = [b'N', b'E', b'S', 0x1A];
const HEADER_SIZE_INES: usize = 16;
#[allow(dead_code)]
const HEADER_SIZE_NES20: usize = 16;
const TRAINER_SIZE: usize = 512;

/// 解析 ROM 数据
pub fn parse_rom(data: &[u8]) -> Result<NesRom, RomError> {
    if data.len() < HEADER_SIZE_INES {
        return Err(RomError::TooSmall(data.len()));
    }

    // 验证签名
    if data[0..4] != INES_SIGNATURE {
        return Err(RomError::InvalidSignature);
    }

    let byte_7 = data[7];
    let byte_15 = data[15];

    // NES 2.0 检测: byte 7 bit 2..0 = 0b1010
    let is_nes20 = (byte_7 & 0x0C) == 0x08;

    let format = if is_nes20 {
        RomFormat::Nes20
    } else {
        RomFormat::Ines
    };

    let prg_units = data[4] as usize;
    let chr_units = data[5] as usize;

    // NES 2.0 使用扩展的 PRG/CHR 大小
    let (prg_size, chr_size) = if is_nes20 {
        let prg_hi = ((byte_7 >> 2) & 0x03) as usize;
        let chr_hi = ((byte_15 >> 2) & 0x03) as usize;
        let prg_mult = 16 * 1024; // 最小单位 16KB
        let chr_mult = 8 * 1024; // 最小单位 8KB
        let prg = if prg_units == 0 {
            // NES 2.0: 0 表示动态大小，由 PRG_HI 决定
            (prg_hi + 1) * prg_mult
        } else {
            prg_units * prg_mult
        };
        let chr = if chr_units == 0 {
            (chr_hi + 1) * chr_mult
        } else {
            chr_units * chr_mult
        };
        (prg, chr)
    } else {
        (prg_units * 16_384, chr_units * 8_192)
    };

    let mapper_id = ((data[7] & 0xF0) | (data[6] >> 4)) as u16;
    let submapper_id = if is_nes20 { data[8] >> 4 } else { 0 };
    let mirroring_byte = data[6] & 0x01;
    let hw_mirroring = (data[6] >> 3) & 0x01 != 0;
    let mirroring = if hw_mirroring {
        // 由硬件/4-screen 决定
        if data[6] & 0x08 != 0 {
            Mirroring::FourScreen
        } else {
            match mirroring_byte {
                0 => Mirroring::Horizontal,
                _ => Mirroring::Vertical,
            }
        }
    } else {
        match mirroring_byte {
            0 => Mirroring::Horizontal,
            _ => Mirroring::Vertical,
        }
    };

    let has_sram = data[6] & 0x02 != 0;
    let has_trainer = data[6] & 0x04 != 0;
    let vs_unisystem = is_nes20 && (data[13] & 0x01 != 0);

    // NES 2.0 console type
    let console_type = if is_nes20 { data[13] } else { 0 };
    let input_type = if is_nes20 { data[14] & 0x03 } else { 0 };

    if has_trainer && is_nes20 {
        // NES 2.0 不允许 trainer
        return Err(RomError::TrainerOnlyInInes);
    }

    let header = RomHeader {
        format,
        prg_rom_size: prg_size,
        chr_rom_size: chr_size,
        mapper_id,
        submapper_id,
        mirroring,
        has_sram,
        has_trainer,
        vs_unisystem,
        console_type,
        input_type,
    };

    let mut offset = HEADER_SIZE_INES;

    let trainer = if has_trainer {
        if data.len() < offset + TRAINER_SIZE {
            return Err(RomError::InvalidFormat("missing trainer data".into()));
        }
        let mut t = [0u8; TRAINER_SIZE];
        t.copy_from_slice(&data[offset..offset + TRAINER_SIZE]);
        offset += TRAINER_SIZE;
        Some(t)
    } else {
        None
    };

    if data.len() < offset + prg_size {
        return Err(RomError::InvalidFormat("missing PRG-ROM data".into()));
    }
    let prg_rom = data[offset..offset + prg_size].to_vec();
    offset += prg_size;

    let (chr_rom, has_chr_ram) = if chr_size == 0 {
        (None, true)
    } else {
        let chr = data[offset..offset + chr_size.min(data.len().saturating_sub(offset))].to_vec();
        (Some(chr), false)
    };

    Ok(NesRom {
        header,
        trainer,
        prg_rom,
        chr_rom,
        has_chr_ram,
        raw: data.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小有效的 iNES ROM header
    fn make_ines(prg_units: u8, chr_units: u8, flags6: u8, flags7: u8) -> Vec<u8> {
        let mut data = vec![b'N', b'E', b'S', 0x1A, prg_units, chr_units, flags6, flags7];
        data.extend_from_slice(&[0u8; 8]); // bytes 8-15
        // PRG data
        data.extend(std::iter::repeat(0u8).take(16_384 * prg_units as usize));
        // CHR data
        data.extend(std::iter::repeat(0u8).take(8_192 * chr_units as usize));
        data
    }

    #[test]
    fn test_parse_ines_nrom() {
        let data = make_ines(1, 1, 0x00, 0x00); // Mapper 0, horizontal, 16KB PRG, 8KB CHR
        let rom = parse_rom(&data).unwrap();
        assert_eq!(rom.header.mapper_id, 0);
        assert_eq!(rom.header.prg_rom_size, 16_384);
        assert_eq!(rom.header.chr_rom_size, 8_192);
        assert_eq!(rom.header.mirroring, Mirroring::Horizontal);
        assert!(!rom.header.has_sram);
        assert_eq!(rom.header.format, super::RomFormat::Ines);
    }

    #[test]
    fn test_parse_ines_mapper1() {
        let data = make_ines(2, 1, 0x10, 0x00); // Mapper 1 = (0x10 >> 4) | 0xF0? 这里 flags6 bit 4 = 1 -> mapper bit 4
        // flags6 = 0x10 means mapper low nibble = 0, bit 4 set = mapper bit 4
        // so mapper = (flags7 & 0xF0) | (flags6 >> 4) = 0 | 1 = 1
        // Actually: mapper_id = (flags7 & 0xF0) | (flags6 >> 4) = 0 | 1 = 1
        let rom = parse_rom(&data).unwrap();
        assert_eq!(rom.header.mapper_id, 1);
    }
}
