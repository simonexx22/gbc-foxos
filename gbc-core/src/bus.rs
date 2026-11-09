//! CPU memory bus.
//!
//! The [`Bus`] trait lets the CPU read and write the 16-bit address space
//! without storing references to any other component. [`SystemBus`] is the
//! top-level adapter that routes accesses to the MMU, cartridge, timer, joypad,
//! serial port, and APU.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::joypad::Joypad;
use crate::mmu::Mmu;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

/// The memory interface used by the CPU.
pub trait Bus {
    /// Reads one byte from the address space.
    fn read(&mut self, addr: u16) -> u8;

    /// Writes one byte to the address space.
    fn write(&mut self, addr: u16, value: u8);

    /// Checks and performs a CGB speed switch if prepared.
    /// Returns true if a speed switch was performed (and CPU should not stop).
    fn speed_switch(&mut self) -> bool {
        false
    }
}

/// Routes CPU accesses to the emulated components.
pub(crate) struct SystemBus<'a> {
    pub(crate) mmu: &'a mut Mmu,
    pub(crate) cartridge: &'a mut Cartridge,
    pub(crate) ppu: &'a mut Ppu,
    pub(crate) apu: &'a mut Apu,
    pub(crate) timer: &'a mut Timer,
    pub(crate) joypad: &'a mut Joypad,
    pub(crate) serial: &'a mut Serial,
    pub(crate) cycle_accum: &'a mut u32,
}

impl SystemBus<'_> {
    /// Advances cycle-driven devices by `cpu_cycles` T-cycles.
    pub(crate) fn tick(&mut self, cpu_cycles: u32) {
        if self.timer.tick(cpu_cycles) {
            self.mmu.raise_interrupt(2);
        }

        if self.joypad.take_interrupt() {
            self.mmu.raise_interrupt(4);
        }

        let base_cycles = if self.mmu.is_double_speed() {
            *self.cycle_accum += cpu_cycles;
            let b = *self.cycle_accum / 2;
            *self.cycle_accum %= 2;
            b
        } else {
            *self.cycle_accum = 0;
            cpu_cycles
        };

        if base_cycles > 0 {
            let vram = self.mmu.vram();
            let oam = self.mmu.oam();
            self.ppu.tick(base_cycles, vram, oam);
            self.raise_ppu_interrupts();

            self.apu.tick(base_cycles);
            self.cartridge.tick(base_cycles);

            if self.mmu.is_cgb() && self.ppu.take_hblank_entered() && self.mmu.is_hdma_active() {
                self.copy_dma_block();
                self.mmu.advance_hdma();
            }
        }
    }

    fn raise_ppu_interrupts(&mut self) {
        let interrupts = self.ppu.take_interrupts();

        if interrupts.vblank {
            self.mmu.raise_interrupt(0);
        }

        if interrupts.stat {
            self.mmu.raise_interrupt(1);
        }
    }

    fn oam_dma(&mut self, value: u8) {
        let source = u16::from(value) << 8;

        for offset in 0..0xA0u16 {
            let byte = self.read(source + offset);
            self.mmu.write(0xFE00 + offset, byte);
        }
    }

    fn copy_dma_block(&mut self) {
        let mut src = self.mmu.hdma_source();
        let mut dst = self.mmu.hdma_dest();

        for _ in 0..16 {
            let byte = self.read(src);
            self.mmu.write(dst, byte);
            src = src.wrapping_add(1);
            dst = 0x8000 | ((dst + 1) & 0x1FFF);
        }

        self.mmu.set_hdma_source(src);
        self.mmu.set_hdma_dest(dst);
    }

    fn gdma(&mut self, blocks: u8) {
        for _ in 0..blocks {
            self.copy_dma_block();
        }
    }
}

impl Bus for SystemBus<'_> {
    fn speed_switch(&mut self) -> bool {
        self.mmu.check_speed_switch()
    }

    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF | 0xA000..=0xBFFF => self.cartridge.read(addr),
            0xFF00 => self.joypad.read(),
            0xFF01 => self.serial.read_sb(),
            0xFF02 => self.serial.read_sc(),
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B => self.ppu.read(addr),
            0xFF68..=0xFF6C => self.ppu.read(addr),
            _ => self.mmu.read(addr),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF | 0xA000..=0xBFFF => self.cartridge.write(addr, value),
            0xFF00 => self.joypad.write(value),
            0xFF01 => self.serial.write_sb(value),
            0xFF02 => {
                if self.serial.write_sc(value) {
                    self.mmu.raise_interrupt(3);
                }
            }
            0xFF04..=0xFF07 => self.timer.write(addr, value),
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF40..=0xFF45 | 0xFF47..=0xFF4B => {
                self.ppu.write(addr, value);
                self.raise_ppu_interrupts();
            }
            0xFF46 => {
                self.oam_dma(value);
                self.mmu.write(addr, value);
            }
            0xFF55 if self.mmu.is_cgb() => {
                if let Some(blocks) = self.mmu.write_hdma5(value) {
                    self.gdma(blocks);
                }
            }
            0xFF68..=0xFF6C => {
                self.ppu.write(addr, value);
                self.raise_ppu_interrupts();
            }
            _ => self.mmu.write(addr, value),
        }
    }
}
