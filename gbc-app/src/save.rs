//! Save-file helpers.
//!
//! `gbc-core` only exposes save RAM as bytes; this module is the only place in
//! the frontend that decides where those bytes live on disk.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Returns the save-file path associated with a ROM path.
///
/// The extension is replaced with `.sav`, so `game.gbc` becomes `game.sav`.
pub fn save_path(rom_path: &Path) -> PathBuf {
    let mut path = rom_path.to_path_buf();
    path.set_extension("sav");
    path
}

/// Returns the save-state path associated with a ROM path.
///
/// The extension is replaced with `.state`, so `game.gbc` becomes `game.state`.
pub fn state_path(rom_path: &Path) -> PathBuf {
    let mut path = rom_path.to_path_buf();
    path.set_extension("state");
    path
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
