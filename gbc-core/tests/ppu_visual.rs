//! Headless visual tests: run a ROM for N frames and dump the framebuffer
//! to a PNG file so the output can be inspected manually or verified against
//! reference images.
//!
//! # Usage
//!
//!   cargo test -p gbc-core --test ppu_visual -- --nocapture
//!
//! Output PNGs are written to `target/ppu_frames/` relative to the workspace
//! root (overwriting on each run).
//!
//! Reference images:
//! * dmg-acid2: <https://github.com/mattcurrie/dmg-acid2>
//! * cgb-acid2: <https://github.com/mattcurrie/cgb-acid2>

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use gbc_core::{Gameboy, SCREEN_HEIGHT, SCREEN_WIDTH};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Directory for all output PNGs (created automatically).
fn out_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().unwrap_or(manifest);
    let dir = workspace.join("target").join("ppu_frames");
    fs::create_dir_all(&dir).expect("failed to create target/ppu_frames/");
    dir
}

/// Run a ROM for `frames` full frames and return the final Gameboy state.
fn run_frames(rom: Vec<u8>, frames: u32) -> Gameboy {
    let mut gb = Gameboy::new(rom);
    for _ in 0..frames {
        gb.step_frame();
    }
    gb
}

/// Write an RGBA8888 framebuffer to a PNG at `path`.
fn dump_png(path: &Path, fb: &[u8]) {
    assert_eq!(
        fb.len(),
        SCREEN_WIDTH * SCREEN_HEIGHT * 4,
        "framebuffer length mismatch"
    );

    let file =
        fs::File::create(path).unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));

    let mut enc = png::Encoder::new(file, SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);

    let mut writer = enc
        .write_header()
        .unwrap_or_else(|e| panic!("PNG header error for {}: {e}", path.display()));

    writer
        .write_image_data(fb)
        .unwrap_or_else(|e| panic!("PNG data error for {}: {e}", path.display()));

    println!("  → {}", path.display());
}

/// Decodes a PNG file to a raw byte buffer (expanded palette).
fn load_png_raw(path: &Path) -> (u32, u32, usize, Vec<u8>) {
    let file = File::open(path)
        .unwrap_or_else(|e| panic!("cannot open reference {}: {e}", path.display()));
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder
        .read_info()
        .unwrap_or_else(|e| panic!("PNG read_info for {}: {e}", path.display()));
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .unwrap_or_else(|e| panic!("PNG next_frame for {}: {e}", path.display()));
    buf.truncate(info.buffer_size());
    let bpp = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => 1,
    };
    (info.width, info.height, bpp, buf)
}

/// Load a ROM from the `tests/roms/` directory.
fn rom_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("roms")
        .join(name)
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// dmg-acid2 (DMG mode).
///
/// Runs the ROM, dumps the framebuffer to PNG, and verifies 0 pixel differences
/// against the official reference image when present.
#[test]
fn dmg_acid2() {
    let path = rom_path("dmg-acid2.gb");
    assert!(path.exists(), "missing ROM: {path:?}");

    let rom = fs::read(&path).expect("cannot read dmg-acid2.gb");
    println!("dmg_acid2: running 120 frames…");
    let gb = run_frames(rom, 120);

    let out = out_dir().join("dmg-acid2.png");
    dump_png(&out, gb.framebuffer());
    println!("dmg_acid2: inspect {}", out.display());

    // Verify against reference image
    let ref_path = rom_path("reference-dmg-acid2.png");
    assert!(ref_path.exists(), "missing reference image: {ref_path:?}");
    let (w, h, bpp, ref_data) = load_png_raw(&ref_path);
    assert_eq!(w, SCREEN_WIDTH as u32);
    assert_eq!(h, SCREEN_HEIGHT as u32);

    let dmg_rgba = [
        [0xE0, 0xF8, 0xD0],
        [0x88, 0xC0, 0x70],
        [0x34, 0x68, 0x56],
        [0x08, 0x18, 0x20],
    ];

    let fb = gb.framebuffer();
    let mut diff_count = 0;
    for idx in 0..SCREEN_WIDTH * SCREEN_HEIGHT {
        let act_rgb = [fb[idx * 4], fb[idx * 4 + 1], fb[idx * 4 + 2]];
        let act_shade = dmg_rgba.iter().position(|&c| c == act_rgb).unwrap_or(99);
        let ref_val = ref_data[idx * bpp];
        let ref_shade = match ref_val {
            255 => 0,
            170 => 1,
            85 => 2,
            0 => 3,
            _ => 98,
        };
        if act_shade != ref_shade {
            diff_count += 1;
        }
    }
    println!(
        "dmg_acid2: pixel differences: {diff_count} / {}",
        SCREEN_WIDTH * SCREEN_HEIGHT
    );
    assert_eq!(
        diff_count, 0,
        "dmg-acid2 has {diff_count} pixel differences from reference"
    );
}

/// cgb-acid2 (CGB mode).
///
/// Runs the ROM, dumps the framebuffer to PNG, and verifies 0 pixel differences
/// against the official reference image when present.
#[test]
fn cgb_acid2() {
    let path = rom_path("cgb-acid2.gbc");
    assert!(path.exists(), "missing ROM: {path:?}");

    let rom = fs::read(&path).expect("cannot read cgb-acid2.gbc");
    println!("cgb_acid2: running 120 frames…");
    let gb = run_frames(rom, 120);

    let out = out_dir().join("cgb-acid2.png");
    dump_png(&out, gb.framebuffer());
    println!("cgb_acid2: inspect {}", out.display());

    // Verify against reference image
    let ref_path = rom_path("reference-cgb-acid2.png");
    assert!(ref_path.exists(), "missing reference image: {ref_path:?}");
    let (w, h, bpp, ref_data) = load_png_raw(&ref_path);
    assert_eq!(w, SCREEN_WIDTH as u32);
    assert_eq!(h, SCREEN_HEIGHT as u32);

    let fb = gb.framebuffer();
    let mut diff_count = 0;
    for idx in 0..SCREEN_WIDTH * SCREEN_HEIGHT {
        let act_rgb = &fb[idx * 4..idx * 4 + 3];
        let ref_rgb = &ref_data[idx * bpp..idx * bpp + 3];
        if act_rgb != ref_rgb {
            diff_count += 1;
        }
    }
    println!(
        "cgb_acid2: pixel differences: {diff_count} / {}",
        SCREEN_WIDTH * SCREEN_HEIGHT
    );
    assert_eq!(
        diff_count, 0,
        "cgb-acid2 has {diff_count} pixel differences from reference"
    );
}

/// Generic helper: run any ROM file for a given number of frames and dump it.
fn visual_snapshot(rom_name: &str, frames: u32, out_name: &str) {
    let path = rom_path(rom_name);
    if !path.exists() {
        eprintln!("SKIP: {rom_name} not found — place it in gbc-core/tests/roms/ to enable");
        return;
    }

    let rom = fs::read(&path).unwrap_or_else(|e| panic!("cannot read {rom_name}: {e}"));
    println!("{out_name}: running {frames} frames…");
    let gb = run_frames(rom, frames);

    let out = out_dir().join(out_name);
    dump_png(&out, gb.framebuffer());
}

/// Tetris (DMG). Place `tetris.gb` in `gbc-core/tests/roms/` to enable.
#[test]
fn tetris_title() {
    visual_snapshot("tetris.gb", 180, "tetris-title.png");
}

/// Any CGB game. Place `starfall.gbc`, `game.gbc`, etc. in `gbc-core/tests/roms/` to enable.
#[test]
fn cgb_game_title() {
    for name in &[
        "starfall.gbc",
        "game.gbc",
        "links-awakening.gbc",
        "pokemon-crystal.gbc",
    ] {
        let path = rom_path(name);
        if path.exists() {
            visual_snapshot(name, 180, "cgb-game-title.png");
            return;
        }
    }
    eprintln!(
        "SKIP: no CGB game ROM found — place starfall.gbc (or game.gbc) in gbc-core/tests/roms/"
    );
}
