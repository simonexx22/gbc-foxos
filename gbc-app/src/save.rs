//! Save-file and ROM header helpers.
//!
//! Handles save RAM and save state locations based on configuration,
//! and parsing Game Boy cartridge metadata directly from files.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::config::{config_dir, PathsConfig, SaveDirectoryMode};

/// Returns the save-file path associated with a ROM path and configuration.
pub fn save_path(rom_path: &Path, paths_config: &PathsConfig) -> PathBuf {
    match paths_config.save_directory_mode {
        SaveDirectoryMode::RomDirectory => {
            let mut path = rom_path.to_path_buf();
            path.set_extension("sav");
            path
        }
        SaveDirectoryMode::ConfigDirectory => {
            let mut path = config_dir();
            path.push("saves");
            let stem = rom_path.file_stem().unwrap_or_default();
            path.push(stem);
            path.set_extension("sav");
            path
        }
    }
}

/// Returns the save-state path associated with a ROM path and configuration.
pub fn state_path(rom_path: &Path, paths_config: &PathsConfig) -> PathBuf {
    match paths_config.save_directory_mode {
        SaveDirectoryMode::RomDirectory => {
            let mut path = rom_path.to_path_buf();
            path.set_extension("state");
            path
        }
        SaveDirectoryMode::ConfigDirectory => {
            let mut path = config_dir();
            path.push("saves");
            let stem = rom_path.file_stem().unwrap_or_default();
            path.push(stem);
            path.set_extension("state");
            path
        }
    }
}

/// Reads a save file, if it exists.
pub fn load(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}

/// Writes save RAM to disk.
///
/// Empty save RAM is skipped so cartridges without a battery do not leave
/// empty files behind.
pub fn store(path: &Path, data: &[u8]) -> io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(path, data)
}

/// Parses the cartridge title and CGB capability from the ROM header.
/// Reads only the first 512 bytes of the file for efficiency.
pub fn parse_rom_header(path: &Path) -> Option<(String, bool)> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; 0x150];
    file.read_exact(&mut header).ok()?;

    let title_bytes = &header[0x134..0x143];
    let end = title_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(title_bytes.len());
    let mut title = String::from_utf8_lossy(&title_bytes[..end])
        .trim()
        .to_string();

    if title.is_empty() {
        if let Some(stem) = path.file_stem() {
            title = stem.to_string_lossy().to_string();
        }
    }

    let is_cgb = (header[0x143] & 0x80) != 0;
    Some((title, is_cgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tetris_header() {
        let path = Path::new("../gbc-core/tests/roms/tetris.gb");
        if path.exists() {
            let (title, is_cgb) = parse_rom_header(path).expect("failed to parse tetris.gb");
            assert_eq!(title, "TETRIS");
            assert!(!is_cgb);
        }
    }

    #[test]
    fn test_parse_starfall_cgb_header() {
        let path = Path::new("../gbc-core/tests/roms/starfall.gbc");
        if path.exists() {
            let (title, is_cgb) = parse_rom_header(path).expect("failed to parse starfall.gbc");
            assert!(!title.is_empty());
            assert!(is_cgb);
        }
    }
}
