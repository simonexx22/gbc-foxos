//! Top-level emulator state.
//!
//! [`Gameboy`] owns every component directly. Components do not store
//! references to one another; all coordination happens here.

use crate::apu::Apu;
use crate::bus::SystemBus;
use crate::button::Button;
use crate::cartridge::Cartridge;
use crate::cpu::Cpu;
use crate::joypad::Joypad;
use crate::mmu::Mmu;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::state::{SaveStateError, StateReader, StateWriter};
use crate::timer::Timer;

/// Base T-cycles in one Game Boy frame (59.7275 Hz ≈ 4_194_304 / 70_224).
pub const CYCLES_PER_FRAME: u32 = 70_224;

/// A complete Game Boy Color.
pub struct Gameboy {
    cpu: Cpu,
    mmu: Mmu,
    ppu: Ppu,
    apu: Apu,
    timer: Timer,
    joypad: Joypad,
    cartridge: Cartridge,
    serial: Serial,
    cycle_accum: u32,
}

impl Gameboy {
    /// Creates a new console from a raw ROM image.
    ///
    /// This does not touch the filesystem. The frontend is responsible for
    /// reading the ROM bytes and for loading any save RAM afterwards with
    /// [`Gameboy::set_save_ram`].
    pub fn new(rom: Vec<u8>) -> Self {
        let cartridge = Cartridge::new(rom);
        let cgb = cartridge.is_cgb();
        let mmu = Mmu::new(cgb);

        Self {
            cpu: Cpu::for_system(cgb),
            mmu,
            ppu: Ppu::new(cgb),
            apu: Apu::default(),
            timer: Timer::default(),
            joypad: Joypad::default(),
            cartridge,
            serial: Serial::default(),
            cycle_accum: 0,
        }
    }

    /// Executes one CPU instruction (or services one interrupt) and advances
    /// the cycle-driven devices.
    pub fn step_instruction(&mut self) -> u32 {
        let mut bus = SystemBus {
            mmu: &mut self.mmu,
            cartridge: &mut self.cartridge,
            ppu: &mut self.ppu,
            apu: &mut self.apu,
            timer: &mut self.timer,
            joypad: &mut self.joypad,
            serial: &mut self.serial,
            cycle_accum: &mut self.cycle_accum,
        };

        let cycles = self.cpu.step(&mut bus);
        bus.tick(cycles);
        cycles
    }

    /// Runs approximately one frame worth of cycles (70,224 base dots).
    pub fn step_frame(&mut self) {
        self.apu.clear_samples();
        let mut elapsed_base = 0;

        while elapsed_base < CYCLES_PER_FRAME {
            let is_double = self.mmu.is_double_speed();
            let cpu_cycles = self.step_instruction();
            elapsed_base += if is_double {
                cpu_cycles / 2
            } else {
                cpu_cycles
            };
        }
    }

    /// The current frame as tightly packed RGBA8888 bytes.
    ///
    /// The length is always `SCREEN_WIDTH * SCREEN_HEIGHT * 4`.
    pub fn framebuffer(&self) -> &[u8] {
        self.ppu.framebuffer()
    }

    /// Interleaved stereo `f32` audio samples produced during the most recent
    /// frame.
    pub fn audio_samples(&self) -> &[f32] {
        self.apu.samples()
    }

    /// Presses or releases a button.
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        self.joypad.set_button(button, pressed);
    }

    /// Battery-backed cartridge RAM, or an empty slice when unsupported.
    ///
    /// The frontend persists these bytes as-is.
    pub fn save_ram(&self) -> &[u8] {
        self.cartridge.save_ram()
    }

    /// Replaces the battery-backed cartridge RAM with `data`.
    ///
    /// The frontend calls this after loading a save file.
    pub fn set_save_ram(&mut self, data: &[u8]) {
        self.cartridge.set_save_ram(data);
    }

    /// Cartridge title from header.
    pub fn title(&self) -> String {
        self.cartridge.title()
    }

    /// Whether this is running a Game Boy Color cartridge.
    pub fn is_cgb(&self) -> bool {
        self.cartridge.is_cgb()
    }

    /// Configures the 4 DMG palette colors (RGBA).
    pub fn set_dmg_palette(&mut self, palette: [[u8; 4]; 4]) {
        self.ppu.set_dmg_palette(palette);
    }

    /// Enables or disables CGB color correction.
    pub fn set_cgb_color_correction(&mut self, enabled: bool) {
        self.ppu.set_cgb_color_correction(enabled);
    }

    /// Enables or disables sound generation for an APU channel (1-4).
    pub fn set_channel_enabled(&mut self, channel: usize, enabled: bool) {
        self.apu.set_channel_enabled(channel, enabled);
    }

    /// Resets the console state, preserving the ROM and save RAM.
    pub fn reset(&mut self) {
        let rom = self.cartridge.rom().to_vec();
        let save = self.cartridge.save_ram().to_vec();
        let mut new_gb = Gameboy::new(rom);
        if !save.is_empty() {
            new_gb.set_save_ram(&save);
        }
        *self = new_gb;
    }

    /// All serial output captured so far.
    ///
    /// Blargg's test ROMs use the serial port to report results, so this is
    /// also useful to a headless test harness.
    pub fn serial_output(&self) -> &[u8] {
        self.serial.output()
    }

    /// Removes and returns all serial output captured so far.
    pub fn take_serial_output(&mut self) -> Vec<u8> {
        self.serial.take_output()
    }

    /// Serializes the entire console state into a byte vector.
    pub fn save_state(&self) -> Vec<u8> {
        let mut w = StateWriter::new();
        self.cpu.save_state(&mut w);
        self.mmu.save_state(&mut w);
        self.ppu.save_state(&mut w);
        self.timer.save_state(&mut w);
        self.joypad.save_state(&mut w);
        self.serial.save_state(&mut w);
        self.cartridge.save_state(&mut w);
        self.apu.save_state(&mut w);
        w.write_u32(self.cycle_accum);
        w.finish()
    }

    /// Restores the console state from serialized bytes.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), SaveStateError> {
        let mut r = StateReader::new(data)?;
        self.cpu.load_state(&mut r)?;
        self.mmu.load_state(&mut r)?;
        self.ppu.load_state(&mut r)?;
        self.timer.load_state(&mut r)?;
        self.joypad.load_state(&mut r)?;
        self.serial.load_state(&mut r)?;
        self.cartridge.load_state(&mut r)?;
        self.apu.load_state(&mut r)?;
        self.cycle_accum = r.read_u32()?;
        Ok(())
    }
}
