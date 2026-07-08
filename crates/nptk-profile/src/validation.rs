use crate::profile::GameProfile;

/// Errors that can occur during profile validation.
#[derive(Debug)]
pub enum ProfileError {
    /// The ROM's mapper number does not match the profile's expected mapper.
    RomMismatch {
        expected_mapper: u16,
        actual_mapper: u16,
    },
    /// General profile validity error.
    InvalidProfile(String),
}

impl core::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProfileError::RomMismatch {
                expected_mapper,
                actual_mapper,
            } => write!(
                f,
                "mapper mismatch: expected {}, got {}",
                expected_mapper, actual_mapper
            ),
            ProfileError::InvalidProfile(msg) => write!(f, "invalid profile: {}", msg),
        }
    }
}

impl std::error::Error for ProfileError {}

// ponytail: single error type for now, split into per-section variants when
// callers need to handle specific fields differently.

/// Check that a ROM's basic header fields are compatible with a `GameProfile`.
///
/// Returns `Ok(())` when all fields match; returns `Err(ProfileError)` on the
/// first mismatch.
pub fn validate_rom_against_profile(
    prg_size: usize,
    chr_size: usize,
    mapper: u16,
    mirroring: u8,
    profile: &GameProfile,
) -> Result<(), ProfileError> {
    // Map mirroring nibble to a human-readable string so we can compare
    // against `profile.rom.mirroring`.
    let mirroring_str = match mirroring {
        0 => "horizontal",
        1 => "vertical",
        _ => "unknown",
    };

    // Mapper check (most critical mismatch)
    if mapper != profile.rom.mapper {
        return Err(ProfileError::RomMismatch {
            expected_mapper: profile.rom.mapper,
            actual_mapper: mapper,
        });
    }

    // PRG-ROM size check
    if prg_size != profile.rom.prg_size {
        return Err(ProfileError::InvalidProfile(format!(
            "PRG-ROM size mismatch: expected {} KiB, got {} KiB",
            profile.rom.prg_size, prg_size,
        )));
    }

    // CHR-ROM size check
    if chr_size != profile.rom.chr_size {
        return Err(ProfileError::InvalidProfile(format!(
            "CHR-ROM size mismatch: expected {} KiB, got {} KiB",
            profile.rom.chr_size, chr_size,
        )));
    }

    // Mirroring check
    if mirroring_str != profile.rom.mirroring {
        return Err(ProfileError::InvalidProfile(format!(
            "mirroring mismatch: expected '{}', got '{}'",
            profile.rom.mirroring, mirroring_str,
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::*;

    fn sample_profile() -> GameProfile {
        GameProfile {
            game: GameSection::default(),
            rom: RomSection {
                mapper: 1,
                prg_size: 128,
                chr_size: 64,
                mirroring: String::from("vertical"),
                ..Default::default()
            },
            cpu: CpuSection::default(),
            ppu: PpuSection::default(),
            audio: AudioSection::default(),
            input: InputSection::default(),
            testing: TestingSection::default(),
        }
    }

    #[test]
    fn valid_rom_passes() {
        let profile = sample_profile();
        let result = validate_rom_against_profile(128, 64, 1, 1, &profile);
        assert!(result.is_ok());
    }

    #[test]
    fn mapper_mismatch_fails() {
        let profile = sample_profile();
        let result = validate_rom_against_profile(128, 64, 99, 1, &profile);
        assert!(matches!(result, Err(ProfileError::RomMismatch { .. })));
    }

    #[test]
    fn prg_size_mismatch_fails() {
        let profile = sample_profile();
        let result = validate_rom_against_profile(256, 64, 1, 1, &profile);
        assert!(result.is_err());
    }

    #[test]
    fn mirroring_mismatch_fails() {
        let profile = sample_profile();
        let result = validate_rom_against_profile(128, 64, 1, 0, &profile);
        assert!(result.is_err());
    }
}
