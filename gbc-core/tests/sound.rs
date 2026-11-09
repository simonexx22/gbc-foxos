//! Headless Blargg `dmg_sound` test suite.
//!
//! Verifies the APU frame sequencer (512 Hz), length counters, sweep,
//! envelope, and wave channel behavior against Blargg's test ROMs.

use std::fs;
use std::path::{Path, PathBuf};

use gbc_core::Gameboy;

#[test]
fn blargg_dmg_sound_all() {
    let rom_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join("dmg_sound");
    assert!(rom_dir.exists(), "missing ROM directory: {rom_dir:?}");

    let mut roms = rom_files(&rom_dir);
    assert!(!roms.is_empty(), "no ROMs found in {rom_dir:?}");
    roms.sort();

    let mut failures = Vec::new();

    for rom in roms {
        let name = rom
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| rom.display().to_string());

        let bytes = fs::read(&rom).unwrap_or_else(|e| panic!("failed to read {name}: {e}"));
        let mut gb = Gameboy::new(bytes);

        // Step up to 300 frames (~5 seconds of Game Boy time)
        let mut passed = false;
        let mut outcome = String::new();

        for _ in 0..300 {
            gb.step_frame();
            let state = gb.save_state();

            if let Some(pos) = state.windows(6).position(|w| w == b"Passed") {
                passed = true;
                let start = pos.saturating_sub(20);
                outcome = String::from_utf8_lossy(&state[start..pos + 6])
                    .trim()
                    .to_string();
                break;
            }

            if let Some(pos) = state.windows(6).position(|w| w == b"Failed") {
                let end = (pos + 20).min(state.len());
                outcome = String::from_utf8_lossy(&state[pos..end]).trim().to_string();
                break;
            }
        }

        if passed {
            println!("  [PASS] {name}");
        } else {
            eprintln!("  [FAIL] {name}: {outcome}");
            failures.push(format!("{name}: {outcome}"));
        }
    }

    assert!(
        failures.is_empty(),
        "Blargg dmg_sound failures:\n{}",
        failures.join("\n")
    );
}

fn rom_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("gb"))
        .collect()
}
