//! Memory management unit.
//!
//! Owns the internal address space: two CGB VRAM banks, eight CGB WRAM banks,
//! OAM, I/O registers, HRAM, and the IE register. Cartridge accesses and the
//! timer/serial/joypad registers are routed by [`crate::bus::SystemBus`], so
//! components never store references to one another.

use crate::state::{SaveStateError, StateReader, StateWriter};

/// Size of the SM83 address space.
pub const ADDRESS_SPACE: usize = 0x1_0000;

/// Size of one VRAM bank in bytes.
pub const VRAM_BANK_SIZE: usize = 0x2000;

/// Number of VRAM banks on the Game Boy Color.
pub const VRAM_BANKS: usize = 2;

/// Size of one WRAM bank in bytes.
pub const WRAM_BANK_SIZE: usize = 0x1000;

/// Number of WRAM banks on the Game Boy Color.
pub const WRAM_BANKS: usize = 8;

const OAM_SIZE: usize = 0xA0;
const IO_SIZE: usize = 0x80;
const HRAM_SIZE: usize = 0x7F;

const IO_BASE: u16 = 0xFF00;
const HRAM_BASE: u16 = 0xFF80;

const IO_IF_ADDR: u16 = 0xFF0F;
const IO_VBK: u16 = 0xFF4F;
const IO_SVBK: u16 = 0xFF70;
const IO_BOOT: u16 = 0xFF50;
const IO_IF: usize = 0x0F;

/// The 64 KiB address space visible to the CPU.
#[derive(Debug)]
pub struct Mmu {
    /// CGB VRAM, two banks of 8 KiB.
    vram: Vec<u8>,
    /// CGB WRAM, eight banks of 4 KiB.
    wram: Vec<u8>,
    oam: [u8; OAM_SIZE],
    io: [u8; IO_SIZE],
    hram: [u8; HRAM_SIZE],
    ie: u8,
    vram_bank: u8,
    wram_bank: u8,
    key1: u8,
    hdma_src: u16,
    hdma_dst: u16,
    hdma_active: bool,
    hdma_blocks: u8,
    cgb: bool,
    boot_rom_enabled: bool,
}

impl Mmu {
    /// Creates an MMU in DMG or CGB mode.
    pub fn new(cgb: bool) -> Self {
        Self {
            vram: vec![0; VRAM_BANK_SIZE * VRAM_BANKS],
            wram: vec![0; WRAM_BANK_SIZE * WRAM_BANKS],
            oam: [0; OAM_SIZE],
            io: [0; IO_SIZE],
            hram: [0; HRAM_SIZE],
            ie: 0,
            vram_bank: 0,
            wram_bank: 0,
            key1: 0,
            hdma_src: 0,
            hdma_dst: 0x8000,
            hdma_active: false,
            hdma_blocks: 0,
            cgb,
            boot_rom_enabled: true,
        }
    }

    /// Whether this MMU is in Game Boy Color mode.
    pub fn is_cgb(&self) -> bool {
        self.cgb
    }

    /// The complete VRAM contents, both CGB banks back to back.
    pub(crate) fn vram(&self) -> &[u8] {
        &self.vram
    }

    /// The OAM contents.
    pub(crate) fn oam(&self) -> &[u8] {
        &self.oam
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_bytes(&self.vram);
        w.write_bytes(&self.wram);
        w.write_raw(&self.oam);
        w.write_raw(&self.io);
        w.write_raw(&self.hram);
        w.write_u8(self.ie);
        w.write_u8(self.vram_bank);
        w.write_u8(self.wram_bank);
        w.write_u8(self.key1);
        w.write_u16(self.hdma_src);
        w.write_u16(self.hdma_dst);
        w.write_bool(self.hdma_active);
        w.write_u8(self.hdma_blocks);
        w.write_bool(self.cgb);
        w.write_bool(self.boot_rom_enabled);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        let vram = r.read_bytes()?;
        if vram.len() != self.vram.len() {
            return Err(SaveStateError::InvalidData("vram size mismatch"));
        }
        self.vram.copy_from_slice(vram);

        let wram = r.read_bytes()?;
        if wram.len() != self.wram.len() {
            return Err(SaveStateError::InvalidData("wram size mismatch"));
        }
        self.wram.copy_from_slice(wram);

        let oam = r.read_raw(OAM_SIZE)?;
        self.oam.copy_from_slice(oam);

        let io = r.read_raw(IO_SIZE)?;
        self.io.copy_from_slice(io);

        let hram = r.read_raw(HRAM_SIZE)?;
        self.hram.copy_from_slice(hram);

        self.ie = r.read_u8()?;
        self.vram_bank = r.read_u8()?;
        self.wram_bank = r.read_u8()?;
        self.key1 = r.read_u8()?;
        self.hdma_src = r.read_u16()?;
        self.hdma_dst = r.read_u16()?;
        self.hdma_active = r.read_bool()?;
        self.hdma_blocks = r.read_u8()?;
        self.cgb = r.read_bool()?;
        self.boot_rom_enabled = r.read_bool()?;
        Ok(())
    }

    /// Reads an address handled by the internal memory/IO map.
    pub(crate) fn read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => self.read_vram(addr),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize],
            0xD000..=0xDFFF => self.read_wram_bank(self.selected_wram_bank(), addr),
            0xE000..=0xFDFF => self.read(addr - 0x2000),
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize],
            0xFEA0..=0xFEFF => 0x00,
            0xFF00..=0xFF7F => self.read_io(addr),
            0xFF80..=0xFFFE => self.hram[(addr - HRAM_BASE) as usize],
            0xFFFF => self.ie,
            _ => 0xFF,
        }
    }

    /// Writes an address handled by the internal memory/IO map.
    pub(crate) fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.write_vram(addr, value),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize] = value,
            0xD000..=0xDFFF => {
                let bank = self.selected_wram_bank();
                self.write_wram_bank(bank, addr, value);
            }
            0xE000..=0xFDFF => self.write(addr - 0x2000, value),
            0xFE00..=0xFE9F => self.oam[(addr - 0xFE00) as usize] = value,
            0xFEA0..=0xFEFF => {}
            0xFF00..=0xFF7F => self.write_io(addr, value),
            0xFF80..=0xFFFE => self.hram[(addr - HRAM_BASE) as usize] = value,
            0xFFFF => self.ie = value,
            _ => {}
        }
    }

    /// Checks and performs a CGB speed switch if prepared.
    pub(crate) fn check_speed_switch(&mut self) -> bool {
        if self.cgb && (self.key1 & 0x01 != 0) {
            self.key1 = (self.key1 ^ 0x80) & 0x80;
            true
        } else {
            false
        }
    }

    /// Whether the CPU is currently operating in double speed mode.
    pub(crate) fn is_double_speed(&self) -> bool {
        self.cgb && (self.key1 & 0x80 != 0)
    }

    /// Whether an HDMA transfer is currently active.
    pub(crate) fn is_hdma_active(&self) -> bool {
        self.hdma_active
    }

    /// Number of 16-byte blocks remaining in the active HDMA transfer.
    #[cfg(test)]
    pub(crate) fn hdma_blocks(&self) -> u8 {
        self.hdma_blocks
    }

    /// Advances the active HDMA transfer by one block.
    pub(crate) fn advance_hdma(&mut self) {
        if self.hdma_blocks > 0 {
            self.hdma_blocks -= 1;
            if self.hdma_blocks == 0 {
                self.hdma_active = false;
            }
        }
    }

    /// Writes to HDMA5 (`0xFF55`).
    ///
    /// If GDMA is initiated, returns `Some(blocks)` to transfer immediately.
    /// If HDMA is started or active HDMA is cancelled, returns `None`.
    pub(crate) fn write_hdma5(&mut self, value: u8) -> Option<u8> {
        if !self.cgb {
            return None;
        }

        if self.hdma_active && value & 0x80 == 0 {
            // Any write with bit 7 = 0 terminates an active HDMA.
            self.hdma_active = false;
            self.hdma_blocks = 0;
            return None;
        }

        let blocks = (value & 0x7F) + 1;
        if value & 0x80 != 0 {
            // Start HDMA
            self.hdma_active = true;
            self.hdma_blocks = blocks;
            None
        } else {
            // Start GDMA
            self.hdma_active = false;
            self.hdma_blocks = 0;
            Some(blocks)
        }
    }

    pub(crate) fn hdma_source(&self) -> u16 {
        self.hdma_src
    }

    pub(crate) fn set_hdma_source(&mut self, src: u16) {
        self.hdma_src = src;
    }

    pub(crate) fn hdma_dest(&self) -> u16 {
        self.hdma_dst
    }

    pub(crate) fn set_hdma_dest(&mut self, dst: u16) {
        self.hdma_dst = dst;
    }

    /// Sets one bit in the interrupt flag register (`FF0F`).
    pub(crate) fn raise_interrupt(&mut self, bit: u8) {
        self.io[IO_IF] |= 1 << bit;
    }

    /// Reads the interrupt flag register (`FF0F`).
    #[cfg(test)]
    pub(crate) fn interrupt_flags(&self) -> u8 {
        self.io[IO_IF]
    }

    /// Reads the interrupt enable register (`FFFF`).
    #[cfg(test)]
    pub(crate) fn interrupt_enable(&self) -> u8 {
        self.ie
    }

    fn selected_vram_bank(&self) -> usize {
        if self.cgb {
            (self.vram_bank & 0x01) as usize
        } else {
            0
        }
    }

    fn selected_wram_bank(&self) -> usize {
        if self.cgb {
            let bank = (self.wram_bank & 0x07) as usize;
            if bank == 0 {
                1
            } else {
                bank
            }
        } else {
            1
        }
    }

    fn read_vram(&self, addr: u16) -> u8 {
        let bank = self.selected_vram_bank();
        let offset = (addr - 0x8000) as usize;
        self.vram[bank * VRAM_BANK_SIZE + offset]
    }

    fn write_vram(&mut self, addr: u16, value: u8) {
        let bank = self.selected_vram_bank();
        let offset = (addr - 0x8000) as usize;
        self.vram[bank * VRAM_BANK_SIZE + offset] = value;
    }

    fn read_wram_bank(&self, bank: usize, addr: u16) -> u8 {
        let offset = (addr - 0xD000) as usize;
        self.wram[bank * WRAM_BANK_SIZE + offset]
    }

    fn write_wram_bank(&mut self, bank: usize, addr: u16, value: u8) {
        let offset = (addr - 0xD000) as usize;
        self.wram[bank * WRAM_BANK_SIZE + offset] = value;
    }

    fn read_io(&self, addr: u16) -> u8 {
        match addr {
            IO_IF_ADDR => self.io[IO_IF] | 0xE0,
            0xFF4D => {
                if self.cgb {
                    self.key1 | 0x7E
                } else {
                    0xFF
                }
            }
            IO_VBK => {
                if self.cgb {
                    self.vram_bank | 0xFE
                } else {
                    0xFF
                }
            }
            0xFF51..=0xFF54 => 0xFF,
            0xFF55 => {
                if self.cgb {
                    if self.hdma_active {
                        (self.hdma_blocks.saturating_sub(1)) & 0x7F
                    } else {
                        0xFF
                    }
                } else {
                    0xFF
                }
            }
            IO_SVBK => {
                if self.cgb {
                    self.wram_bank | 0xF8
                } else {
                    0xFF
                }
            }
            _ => self.io[(addr - IO_BASE) as usize],
        }
    }

    fn write_io(&mut self, addr: u16, value: u8) {
        match addr {
            IO_IF_ADDR => self.io[IO_IF] = value & 0x1F,
            0xFF4D => {
                if self.cgb {
                    self.key1 = (self.key1 & 0x80) | (value & 0x01);
                }
            }
            IO_VBK => {
                if self.cgb {
                    self.vram_bank = value & 0x01;
                }
            }
            0xFF51 => {
                if self.cgb {
                    self.hdma_src = (self.hdma_src & 0x00FF) | (u16::from(value) << 8);
                }
            }
            0xFF52 => {
                if self.cgb {
                    self.hdma_src = (self.hdma_src & 0xFF00) | u16::from(value & 0xF0);
                }
            }
            0xFF53 => {
                if self.cgb {
                    self.hdma_dst =
                        (self.hdma_dst & 0x00FF) | (u16::from(value & 0x1F | 0x80) << 8);
                }
            }
            0xFF54 => {
                if self.cgb {
                    self.hdma_dst = (self.hdma_dst & 0xFF00) | u16::from(value & 0xF0);
                }
            }
            0xFF55 => {
                let _ = self.write_hdma5(value);
            }
            IO_SVBK => {
                if self.cgb {
                    self.wram_bank = value & 0x07;
                }
            }
            IO_BOOT => {
                self.io[(addr - IO_BASE) as usize] = value;
                if value != 0 {
                    self.boot_rom_enabled = false;
                }
            }
            _ => self.io[(addr - IO_BASE) as usize] = value,
        }
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgb_vram_banking() {
        let mut mmu = Mmu::new(true);

        mmu.write(0x8000, 0x12);
        mmu.write(IO_VBK, 1);
        assert_eq!(mmu.read(0x8000), 0x00);

        mmu.write(0x8000, 0x34);
        assert_eq!(mmu.read(0x8000), 0x34);

        mmu.write(IO_VBK, 0);
        assert_eq!(mmu.read(0x8000), 0x12);
    }

    #[test]
    fn cgb_wram_banking_and_echo() {
        let mut mmu = Mmu::new(true);

        mmu.write(0xD000, 0x11);
        mmu.write(IO_SVBK, 2);
        assert_eq!(mmu.read(0xD000), 0x00);

        mmu.write(0xD000, 0x22);
        assert_eq!(mmu.read(0xD000), 0x22);
        assert_eq!(mmu.read(0xF000), 0x22);

        mmu.write(IO_SVBK, 0);
        assert_eq!(mmu.read(0xD000), 0x11);
        assert_eq!(mmu.read(0xF000), 0x11);
    }

    #[test]
    fn dmg_has_one_vram_and_wram_bank() {
        let mut mmu = Mmu::new(false);

        mmu.write(0x8000, 0x42);
        mmu.write(IO_VBK, 1);
        assert_eq!(mmu.read(0x8000), 0x42);

        mmu.write(0xD000, 0x99);
        mmu.write(IO_SVBK, 7);
        assert_eq!(mmu.read(0xD000), 0x99);
        assert_eq!(mmu.read(IO_VBK), 0xFF);
        assert_eq!(mmu.read(IO_SVBK), 0xFF);
    }

    #[test]
    fn io_hram_and_ie_are_separate_regions() {
        let mut mmu = Mmu::new(true);

        mmu.write(0xFF80, 0xAA);
        mmu.write(0xFFFE, 0xBB);
        mmu.write(0xFFFF, 0xCC);
        mmu.write(0xFF10, 0xDD);

        assert_eq!(mmu.read(0xFF80), 0xAA);
        assert_eq!(mmu.read(0xFFFE), 0xBB);
        assert_eq!(mmu.read(0xFFFF), 0xCC);
        assert_eq!(mmu.read(0xFF10), 0xDD);
    }

    #[test]
    fn echo_ram_mirrors_c000_ddff() {
        let mut mmu = Mmu::new(false);

        mmu.write(0xC123, 0x5A);
        assert_eq!(mmu.read(0xE123), 0x5A);

        mmu.write(0xE456, 0xA5);
        assert_eq!(mmu.read(0xC456), 0xA5);
    }

    #[test]
    fn interrupt_flags_live_at_ff0f_and_upper_bits_read_high() {
        let mut mmu = Mmu::new(false);

        mmu.raise_interrupt(2);
        assert_eq!(mmu.interrupt_flags(), 0x04);
        assert_eq!(mmu.read(0xFF0F), 0xE4);

        mmu.write(0xFF0F, 0x00);
        assert_eq!(mmu.interrupt_flags(), 0x00);
        assert_eq!(mmu.read(0xFF0F), 0xE0);

        mmu.write(0xFF0F, 0xFF);
        assert_eq!(mmu.interrupt_flags(), 0x1F);
        assert_eq!(mmu.read(0xFF0F), 0xFF);

        mmu.write(0xFFFF, 0x1F);
        assert_eq!(mmu.interrupt_enable(), 0x1F);
    }

    #[test]
    fn cgb_speed_switch_and_double_speed() {
        let mut mmu = Mmu::new(true);
        assert!(!mmu.is_double_speed());
        assert_eq!(mmu.read(0xFF4D), 0x7E);

        // Arm speed switch
        mmu.write(0xFF4D, 0x01);
        assert_eq!(mmu.read(0xFF4D), 0x7F);

        // Perform speed switch (as triggered by STOP instruction)
        assert!(mmu.check_speed_switch());
        assert!(mmu.is_double_speed());
        assert_eq!(mmu.read(0xFF4D), 0xFE);

        // Calling again without rearming returns false
        assert!(!mmu.check_speed_switch());
        assert!(mmu.is_double_speed());

        // Arm again to return to normal speed
        mmu.write(0xFF4D, 0x01);
        assert_eq!(mmu.read(0xFF4D), 0xFF);
        assert!(mmu.check_speed_switch());
        assert!(!mmu.is_double_speed());
        assert_eq!(mmu.read(0xFF4D), 0x7E);
    }

    #[test]
    fn cgb_hdma_and_gdma_control() {
        let mut mmu = Mmu::new(true);

        mmu.write(0xFF51, 0x1F);
        mmu.write(0xFF52, 0xF5); // Lower 4 bits ignored -> 0x1FF0
        mmu.write(0xFF53, 0x90); // Lower 5 bits -> 0x10, | 0x80 -> 0x9000
        mmu.write(0xFF54, 0x85); // Lower 4 bits ignored -> 0x9080

        assert_eq!(mmu.hdma_source(), 0x1FF0);
        assert_eq!(mmu.hdma_dest(), 0x9080);

        // GDMA trigger (bit 7 = 0)
        let gdma = mmu.write_hdma5(0x07); // 8 blocks
        assert_eq!(gdma, Some(8));
        assert!(!mmu.is_hdma_active());
        assert_eq!(mmu.read(0xFF55), 0xFF);

        // HDMA trigger (bit 7 = 1)
        let hdma = mmu.write_hdma5(0x83); // 4 blocks
        assert_eq!(hdma, None);
        assert!(mmu.is_hdma_active());
        assert_eq!(mmu.hdma_blocks(), 4);
        assert_eq!(mmu.read(0xFF55), 0x03);

        mmu.advance_hdma();
        assert_eq!(mmu.hdma_blocks(), 3);
        assert_eq!(mmu.read(0xFF55), 0x02);

        // Cancel HDMA (write bit 7 = 0 while active)
        let cancel = mmu.write_hdma5(0x00);
        assert_eq!(cancel, None);
        assert!(!mmu.is_hdma_active());
        assert_eq!(mmu.read(0xFF55), 0xFF);
    }
}
