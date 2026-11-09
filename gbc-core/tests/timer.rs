//! Headless timer and instruction-timing test runners.
//!
//! Blargg's `instr_timing` reports ASCII results over the serial port. The
//! Mooneye timer acceptance tests report a result as six bytes over the serial
//! port: `3, 5, 8, 13, 21, 34` on success or six `0x42` bytes on failure.

mod common;

use std::path::Path;

#[test]
fn blargg_instr_timing() {
    let rom = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join("instr_timing")
        .join("instr_timing.gb");

    if !rom.exists() {
        eprintln!(
            "SKIP: instr_timing.gb not found at {} — skipping test",
            rom.display()
        );
        return;
    }

    let (output, cycles) = common::run_blargg(&rom);

    println!("=== instr_timing ({cycles} cycles) ===");
    print!("{output}");
    if !output.ends_with('\n') {
        println!();
    }

    assert!(
        output.contains("Passed") && !output.contains("Failed"),
        "Blargg instr_timing failed: {output:?}"
    );
}

#[test]
fn mooneye_timer_acceptance() {
    let rom_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join("mooneye")
        .join("timer");
    let mut roms = common::rom_files(&rom_dir);

    if roms.is_empty() {
        eprintln!(
            "SKIP: no Mooneye timer ROMs found in {} — skipping test",
            rom_dir.display()
        );
        return;
    }

    roms.sort();
    let mut failures = Vec::new();

    for rom in roms {
        let (output, cycles) = common::run_mooneye(&rom);
        let name = rom
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| rom.display().to_string());

        println!("=== {name} ({cycles} cycles) ===");
        println!("{output:02X?}");

        if output != common::MOONEYE_PASS {
            failures.push(format!("{name}: {output:02X?}"));
        }
    }

    assert!(
        failures.is_empty(),
        "Mooneye timer failures:\n{}",
        failures.join("\n")
    );
}
