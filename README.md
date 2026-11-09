# gbcemu

A Game Boy Color emulator structured as a Cargo workspace.

## Layout

```text
.
├── Cargo.toml          # Workspace manifest
├── gbc-core/           # Platform-independent emulation library
│   └── src/
│       ├── lib.rs      # Public constants and re-exports
│       ├── gameboy.rs  # Gameboy owning all components
│       ├── cpu.rs      # SM83 state
│       ├── mmu.rs      # 64 KiB address space
│       ├── ppu.rs      # LCD and RGBA framebuffer
│       ├── apu.rs      # Audio sample buffer
│       ├── timer.rs    # DIV/TIMA/TMA/TAC state
│       ├── joypad.rs   # Digital input state
│       ├── cartridge.rs# ROM and save RAM
│       └── button.rs   # Button enum
└── gbc-app/            # Desktop frontend
    └── src/
        ├── main.rs     # CLI entry point
        ├── app.rs      # winit + pixels event loop
        ├── audio.rs    # cpal output stream
        ├── input.rs    # Keyboard mapping
        └── save.rs     # Paths and save-file I/O
```

`gbc-core` has no dependencies and performs no I/O. `gbc-app` owns the
filesystem, windowing, and audio concerns.

## Core API

```rust
let mut gb = Gameboy::new(rom_bytes);
gb.step_frame();

let rgba: &[u8] = gb.framebuffer();       // 160 * 144 * 4 bytes
let audio: &[f32] = gb.audio_samples();   // interleaved stereo

gb.set_button(Button::A, true);
gb.set_button(Button::A, false);

let ram: &[u8] = gb.save_ram();
gb.set_save_ram(&ram);
```

## Run

```sh
cargo run -p gbc-app -- path/to/game.gbc
```

The save file is written next to the ROM with a `.sav` extension.

## Tests

The CPU is verified against Blargg's individual `cpu_instrs` ROMs. The test
runs each ROM headlessly, captures serial output, and requires `Passed`:

```sh
cargo test -p gbc-core --test cpu_instrs
```

Instruction timing and timer behaviour are verified with Blargg's
`instr_timing` and all 13 Mooneye timer acceptance tests:

```sh
cargo test -p gbc-core --test timer
```

Interrupt dispatch, the EI delay, DI/EI sequences, RETI and IE/IF register
semantics are verified with selected Mooneye interrupt acceptance tests:

```sh
cargo test -p gbc-core --test interrupts
```

## Emulation status

- Sharp LR35902 CPU: complete instruction set, flags, CB-prefixed opcodes,
  interrupts, EI delay, HALT and the HALT bug.
- Full memory map: VRAM, WRAM, echo RAM, OAM, I/O, HRAM and IE, plus OAM DMA.
- CGB VRAM banking (2 banks) and WRAM banking (8 banks) via `VBK`/`SVBK`.
- ROM-only, MBC1, MBC3 (with RTC) and MBC5 cartridge mapping.
- Timer: DIV, TIMA, TMA and TAC with falling-edge increments, the four-T-cycle
  overflow/reload delay, and TIMA/TMA write behaviour during reload.
- Interrupt system: IE/IF with correct upper-bit reads, timer/serial/joypad
  requests, priority dispatch, the EI one-instruction delay and HALT wake-up.
- PPU rendering, APU synthesis, CGB palettes/HDMA and the remaining cartridge
  controllers are still marked with `TODO`s.
