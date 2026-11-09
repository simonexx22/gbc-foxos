use gbc_core::{Button, Gameboy, SaveStateError};

fn dummy_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    rom[0x0143] = 0x80; // CGB flag
    rom[0x0147] = 0x01; // MBC1
    rom[0x0148] = 0x00; // 32KB ROM
    rom[0x0149] = 0x02; // 8KB RAM
    rom
}

#[test]
fn save_state_roundtrip_preserves_state() {
    let rom = dummy_rom();
    let mut gb = Gameboy::new(rom.clone());

    // Step a few frames to advance PPU, CPU, Timer, APU
    for _ in 0..10 {
        gb.step_frame();
    }

    // Set buttons, cartridge save ram
    gb.set_button(Button::A, true);
    gb.set_button(Button::Start, true);
    gb.set_save_ram(&[0x12, 0x34, 0x56, 0x78]);

    // Step another frame
    gb.step_frame();

    let saved_state = gb.save_state();
    assert!(!saved_state.is_empty());

    // Take snapshot of current state
    let fb_before = gb.framebuffer().to_vec();
    let ram_before = gb.save_ram().to_vec();

    // Advance 5 more frames (diverging state)
    for _ in 0..5 {
        gb.step_frame();
    }
    gb.set_button(Button::A, false);
    gb.set_save_ram(&[0xAA, 0xBB, 0xCC, 0xDD]);

    // Now restore saved state
    gb.load_state(&saved_state)
        .expect("load_state should succeed");

    assert_eq!(gb.framebuffer(), fb_before.as_slice());
    assert_eq!(gb.save_ram(), ram_before.as_slice());
}

#[test]
fn save_state_error_handling() {
    let rom = dummy_rom();
    let mut gb = Gameboy::new(rom);

    // Invalid magic
    let bad_magic = b"WRONG_DATA_AT_START";
    assert_eq!(gb.load_state(bad_magic), Err(SaveStateError::InvalidMagic));

    // Truncated data
    let truncated = b"GBCE_STATE";
    assert_eq!(gb.load_state(truncated), Err(SaveStateError::TruncatedData));
}
