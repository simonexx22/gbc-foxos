//! Headless Blargg `cpu_instrs` runner.
//!
//! Each individual test ROM is executed until it reports a result over the
//! serial port. Success requires the output to contain `Passed`.

use std::fs;
use std::path::{Path, PathBuf};

use gbc_core::Gameboy;

const MAX_CYCLES: u64 = 150_000_000;

#[test]
fn blargg_cpu_instrs() {
    let rom_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join("cpu_instrs");
    assert!(rom_dir.exists(), "missing ROM directory: {rom_dir:?}");

    let mut roms = rom_files(&rom_dir);
    assert!(
        !roms.is_empty(),
        "missing ROM: discovered ROM list in {rom_dir:?} is empty"
    );

    roms.sort();
    let mut failures = Vec::new();

    for rom in roms {
        let (output, cycles) = run_rom(&rom);
        let name = rom
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| rom.display().to_string());

        println!("=== {name} ({cycles} cycles) ===");
        print!("{output}");
        if !output.ends_with('\n') {
            println!();
        }

        let passed = output.contains("Passed") && !output.contains("Failed");
        if !passed {
            failures.push(format!("{name}: {output:?}"));
        }
    }

    assert!(
        failures.is_empty(),
        "Blargg cpu_instrs failures:\n{}",
        failures.join("\n")
    );
}

fn rom_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "gb"))
        .collect()
}

fn run_rom(path: &Path) -> (String, u64) {
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
