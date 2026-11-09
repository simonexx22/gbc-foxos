//! Shared helpers for headless external test-ROM runners.

#![allow(dead_code)] // Each integration test binary uses a different subset.

use std::fs;
use std::path::{Path, PathBuf};

use gbc_core::Gameboy;

pub const MAX_CYCLES: u64 = 400_000_000;

/// The six register values Mooneye's pass/fail protocol sends on success.
pub const MOONEYE_PASS: [u8; 6] = [3, 5, 8, 13, 21, 34];

pub fn rom_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "gb"))
        .collect();
    files.sort();
    files
}

pub fn run_blargg(path: &Path) -> (String, u64) {
    let rom = fs::read(path).expect("failed to read test ROM");
    let mut gameboy = Gameboy::new(rom);
    let mut output = Vec::new();
    let mut cycles = 0;

    while cycles < MAX_CYCLES {
        cycles += u64::from(gameboy.step_instruction());

        let chunk = gameboy.take_serial_output();
        if chunk.is_empty() {
            continue;
        }

        output.extend_from_slice(&chunk);
        let text = String::from_utf8_lossy(&output);
        if text.contains("Passed") || text.contains("Failed") {
            break;
        }
    }

    (String::from_utf8_lossy(&output).into_owned(), cycles)
}

pub fn run_mooneye(path: &Path) -> (Vec<u8>, u64) {
    let rom = fs::read(path).expect("failed to read test ROM");
    let mut gameboy = Gameboy::new(rom);
    let mut output = Vec::new();
    let mut cycles = 0;

    while cycles < MAX_CYCLES && output.len() < MOONEYE_PASS.len() {
        cycles += u64::from(gameboy.step_instruction());
        output.extend_from_slice(&gameboy.take_serial_output());
    }

    (output, cycles)
}
