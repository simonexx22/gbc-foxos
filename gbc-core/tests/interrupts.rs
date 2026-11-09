//! Headless Mooneye interrupt acceptance tests.
//!
//! These ROMs exercise interrupt dispatch timing, the EI delay, repeated EI,
//! DI/EI sequences, RETI, and the IE/IF register semantics. They use the same
//! six-byte serial pass/fail protocol as the timer tests.

mod common;

use std::path::Path;

#[test]
fn mooneye_interrupt_acceptance() {
    let rom_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join("mooneye")
        .join("interrupts");
    assert!(rom_dir.exists(), "missing ROM directory: {rom_dir:?}");

    let mut roms = common::rom_files(&rom_dir);
    assert!(
        !roms.is_empty(),
        "missing ROM: discovered ROM list in {rom_dir:?} is empty"
    );

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
        "Mooneye interrupt failures:\n{}",
        failures.join("\n")
    );
}
