//! Cartridge ROM, external RAM, and memory bank controllers.
//!
//! Implements ROM-only, MBC1, MBC3 (including RTC), and MBC5 cartridges.
//! Cartridge saves are exposed only as byte slices; the frontend owns all file
//! I/O.

use crate::state::{SaveStateError, StateReader, StateWriter};

const ROM_BANK_SIZE: usize = 0x4000;
const RAM_BANK_SIZE: usize = 0x2000;

/// Memory bank controller family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mbc {
    RomOnly,
    Mbc1,
    Mbc3 { rtc: bool },
    Mbc5 { rumble: bool },
    Unsupported,
}

/// Cartridge header information and bank-switching state.
#[derive(Debug)]
pub struct Cartridge {
    rom: Vec<u8>,
    ram: Vec<u8>,
    mbc: Mbc,
    cgb: bool,
    ram_enabled: bool,
    rom_bank: u16,
    ram_bank: u8,
    bank2: u8,
    mode: u8,
    rtc_select: Option<u8>,
    rtc_latch: bool,
    rtc: Rtc,
}

impl Cartridge {
    /// Creates a cartridge from a raw ROM image.
    pub fn new(rom: Vec<u8>) -> Self {
        let cartridge_type = rom.get(0x0147).copied().unwrap_or(0);
        let ram_size = ram_size(rom.get(0x0149).copied().unwrap_or(0));
        let cgb = rom.get(0x0143).copied().unwrap_or(0) & 0x80 != 0;

        let mbc = match cartridge_type {
            0x00 => Mbc::RomOnly,
            0x01..=0x03 => Mbc::Mbc1,
            0x0F..=0x13 => Mbc::Mbc3 {
                rtc: matches!(cartridge_type, 0x0F | 0x10),
            },
            0x19..=0x1E => Mbc::Mbc5 {
                rumble: matches!(cartridge_type, 0x1C..=0x1E),
            },
            _ => Mbc::Unsupported,
        };

        Self {
            rom,
            ram: vec![0; ram_size],
            mbc,
            cgb,
            ram_enabled: false,
            rom_bank: 1,
            ram_bank: 0,
            bank2: 0,
            mode: 0,
            rtc_select: None,
            rtc_latch: false,
            rtc: Rtc::default(),
        }
    }

    /// The raw ROM image.
    pub fn rom(&self) -> &[u8] {
        &self.rom
    }

    /// Whether the cartridge header marks this as a Game Boy Color cartridge.
    pub fn is_cgb(&self) -> bool {
        self.cgb
    }

    /// Title read from the cartridge header (0x0134..=0x0143).
    pub fn title(&self) -> String {
        if self.rom.len() < 0x0144 {
            return String::new();
        }
        let title_bytes = &self.rom[0x0134..0x0143];
        let end = title_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(title_bytes.len());
        String::from_utf8_lossy(&title_bytes[..end])
            .trim()
            .to_string()
    }

    /// Battery-backed cartridge RAM, or an empty slice when the cartridge has
    /// no save RAM.
    ///
    /// The frontend persists this slice as-is.
    pub fn save_ram(&self) -> &[u8] {
        &self.ram
    }

    /// Replaces the cartridge RAM contents with `data`.
    ///
    /// The frontend calls this after loading a save file. No filesystem access
    /// happens here.
    pub fn set_save_ram(&mut self, data: &[u8]) {
        if self.ram.is_empty() || data.is_empty() {
            return;
        }

        self.ram.fill(0);
        let count = data.len().min(self.ram.len());
        self.ram[..count].copy_from_slice(&data[..count]);
    }

    /// Advances cartridge-side time-driven hardware (the MBC3 RTC).
    pub(crate) fn tick(&mut self, cycles: u32) {
        if let Mbc::Mbc3 { rtc: true } = self.mbc {
            self.rtc.tick(cycles);
        }
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_bytes(&self.ram);
        w.write_bool(self.ram_enabled);
        w.write_u16(self.rom_bank);
        w.write_u8(self.ram_bank);
        w.write_u8(self.bank2);
        w.write_u8(self.mode);
        match self.rtc_select {
            Some(val) => {
                w.write_bool(true);
                w.write_u8(val);
            }
            None => {
                w.write_bool(false);
                w.write_u8(0);
            }
        }
        w.write_bool(self.rtc_latch);
        w.write_raw(&self.rtc.registers);
        w.write_raw(&self.rtc.latched);
        w.write_u32(self.rtc.cycles);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        let ram = r.read_bytes()?;
        if ram.len() != self.ram.len() {
            return Err(SaveStateError::InvalidData("cartridge ram size mismatch"));
        }
        self.ram.copy_from_slice(ram);
        self.ram_enabled = r.read_bool()?;
        self.rom_bank = r.read_u16()?;
        self.ram_bank = r.read_u8()?;
        self.bank2 = r.read_u8()?;
        self.mode = r.read_u8()?;
        let has_rtc = r.read_bool()?;
        let rtc_val = r.read_u8()?;
        self.rtc_select = if has_rtc { Some(rtc_val) } else { None };
        self.rtc_latch = r.read_bool()?;
        self.rtc.registers.copy_from_slice(r.read_raw(5)?);
        self.rtc.latched.copy_from_slice(r.read_raw(5)?);
        self.rtc.cycles = r.read_u32()?;
        Ok(())
    }

    /// Reads a cartridge-mapped address.
    pub(crate) fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => {
                let bank = self.fixed_rom_bank();
                self.read_rom(bank, addr as usize)
            }
            0x4000..=0x7FFF => {
                let bank = self.switchable_rom_bank();
                self.read_rom(bank, (addr - 0x4000) as usize)
            }
            0xA000..=0xBFFF => self.read_ram(addr),
            _ => 0xFF,
        }
    }

    /// Writes a cartridge-mapped address.
    pub(crate) fn write(&mut self, addr: u16, value: u8) {
        match self.mbc {
            Mbc::RomOnly | Mbc::Unsupported => {}

            Mbc::Mbc1 => match addr {
                0x0000..=0x1FFF => {
                    self.ram_enabled = value & 0x0F == 0x0A;
                }
                0x2000..=0x3FFF => {
                    self.rom_bank = u16::from(value & 0x1F);
                }
                0x4000..=0x5FFF => {
                    self.bank2 = value & 0x03;
                }
                0x6000..=0x7FFF => {
                    self.mode = value & 0x01;
                }
                0xA000..=0xBFFF => self.write_ram(addr, value),
                _ => {}
            },

            Mbc::Mbc3 { rtc } => match addr {
                0x0000..=0x1FFF => {
                    self.ram_enabled = value & 0x0F == 0x0A;
                }
                0x2000..=0x3FFF => {
                    self.rom_bank = u16::from(value & 0x7F);
                }
                0x4000..=0x5FFF => {
                    if rtc && (0x08..=0x0C).contains(&value) {
                        self.rtc_select = Some(value);
                    } else {
                        self.rtc_select = None;
                        self.ram_bank = value & 0x03;
                    }
                }
                0x6000..=0x7FFF => {
                    if rtc {
                        if value == 0 {
                            self.rtc_latch = false;
                        } else if value == 1 && !self.rtc_latch {
                            self.rtc_latch = true;
                            self.rtc.latch();
                        }
                    }
                }
                0xA000..=0xBFFF => self.write_ram(addr, value),
                _ => {}
            },

            Mbc::Mbc5 { rumble } => match addr {
                0x0000..=0x1FFF => {
                    self.ram_enabled = value & 0x0F == 0x0A;
                }
                0x2000..=0x2FFF => {
                    self.rom_bank = (self.rom_bank & 0x0100) | u16::from(value);
                }
                0x3000..=0x3FFF => {
                    self.rom_bank = (self.rom_bank & 0x00FF) | (u16::from(value & 0x01) << 8);
                }
                0x4000..=0x5FFF => {
                    self.ram_bank = if rumble { value & 0x07 } else { value & 0x0F };
                }
                0xA000..=0xBFFF => self.write_ram(addr, value),
                _ => {}
            },
        }
    }

    fn rom_bank_count(&self) -> usize {
        self.rom.len().div_ceil(ROM_BANK_SIZE).max(1)
    }

    fn fixed_rom_bank(&self) -> usize {
        if self.mbc == Mbc::Mbc1 && self.mode == 1 {
            ((self.bank2 as usize) << 5) % self.rom_bank_count()
        } else {
            0
        }
    }

    fn switchable_rom_bank(&self) -> usize {
        let bank = match self.mbc {
            Mbc::RomOnly | Mbc::Unsupported => 1,
            Mbc::Mbc1 => {
                let low = if self.rom_bank == 0 {
                    1
                } else {
                    self.rom_bank as usize & 0x1F
                };
                let upper = if self.mode == 1 {
                    (self.bank2 as usize) << 5
                } else {
                    0
                };
                upper | low
            }
            Mbc::Mbc3 { .. } => {
                if self.rom_bank == 0 {
                    1
                } else {
                    self.rom_bank as usize & 0x7F
                }
            }
            Mbc::Mbc5 { .. } => self.rom_bank as usize & 0x01FF,
        };

        bank % self.rom_bank_count()
    }

    fn read_rom(&self, bank: usize, offset: usize) -> u8 {
        let index = bank * ROM_BANK_SIZE + offset;
        self.rom.get(index).copied().unwrap_or(0xFF)
    }

    fn ram_bank(&self) -> usize {
        match self.mbc {
            Mbc::Mbc1 => {
                if self.mode == 1 {
                    0
                } else {
                    (self.bank2 & 0x03) as usize
                }
            }
            Mbc::Mbc3 { .. } | Mbc::Mbc5 { .. } => (self.ram_bank & 0x0F) as usize,
            Mbc::RomOnly | Mbc::Unsupported => 0,
        }
    }

    fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled {
            return 0xFF;
        }

        if let Some(register) = self.rtc_select {
            if matches!(self.mbc, Mbc::Mbc3 { rtc: true }) {
                return self.rtc.read(register);
            }
        }

        if self.ram.is_empty() {
            return 0xFF;
        }

        let offset = self.ram_offset(addr);
        self.ram.get(offset).copied().unwrap_or(0xFF)
    }

    fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enabled {
            return;
        }

        if let Some(register) = self.rtc_select {
            if matches!(self.mbc, Mbc::Mbc3 { rtc: true }) {
                self.rtc.write(register, value);
                return;
            }
        }

        if self.ram.is_empty() {
            return;
        }

        let offset = self.ram_offset(addr);
        if let Some(slot) = self.ram.get_mut(offset) {
            *slot = value;
        }
    }

    fn ram_offset(&self, addr: u16) -> usize {
        let offset = self.ram_bank() * RAM_BANK_SIZE + (addr - 0xA000) as usize;
        offset % self.ram.len()
    }
}

fn ram_size(code: u8) -> usize {
    match code {
        0x01 => 2 * 1024,
        0x02 => 8 * 1024,
        0x03 => 32 * 1024,
        0x04 => 128 * 1024,
        0x05 => 64 * 1024,
        _ => 0,
    }
}

/// MBC3 real-time clock registers.
#[derive(Debug, Default)]
struct Rtc {
    /// Live registers: seconds, minutes, hours, day low, day high/flags.
    registers: [u8; 5],
    /// Copy captured by a 0->1 latch write.
    latched: [u8; 5],
    /// T-cycles accumulated toward the next 32768 Hz tick.
    cycles: u32,
}

impl Rtc {
    fn tick(&mut self, cycles: u32) {
        if self.registers[4] & 0x40 != 0 {
            return;
        }

        self.cycles += cycles;

        // The RTC crystal is 32768 Hz and the CPU runs at 4194304 Hz.
        while self.cycles >= 128 {
            self.cycles -= 128;
            self.advance_second();
        }
    }

    fn read(&self, register: u8) -> u8 {
        self.latched[usize::from(register - 0x08)]
    }

    fn write(&mut self, register: u8, value: u8) {
        let index = usize::from(register - 0x08);
        self.registers[index] = value;
        self.latched[index] = value;

        if index == 0 {
            self.cycles = 0;
        }
    }

    fn latch(&mut self) {
        self.latched = self.registers;
    }

    fn advance_second(&mut self) {
        self.registers[0] = self.registers[0].wrapping_add(1);
        if self.registers[0] < 60 {
            return;
        }

        self.registers[0] = 0;
        self.registers[1] = self.registers[1].wrapping_add(1);
        if self.registers[1] < 60 {
            return;
        }

        self.registers[1] = 0;
        self.registers[2] = self.registers[2].wrapping_add(1);
        if self.registers[2] < 24 {
            return;
        }

        self.registers[2] = 0;

        let mut day = u16::from(self.registers[3]) | (u16::from(self.registers[4] & 0x01) << 8);
        day = day.wrapping_add(1);

        if day > 0x01FF {
            day = 0;
            self.registers[4] |= 0x80;
        }

        self.registers[3] = day as u8;
        self.registers[4] = (self.registers[4] & !0x01) | ((day >> 8) as u8 & 0x01);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rom(cartridge_type: u8, rom_banks: usize, ram_code: u8) -> Vec<u8> {
        let mut rom = vec![0; rom_banks * ROM_BANK_SIZE];
        rom[0x0143] = 0x80;
        rom[0x0147] = cartridge_type;
        rom[0x0149] = ram_code;

        for bank in 0..rom_banks {
            rom[bank * ROM_BANK_SIZE] = bank as u8;
        }

        rom
    }

    #[test]
    fn detects_cgb_header() {
        let cartridge = Cartridge::new(test_rom(0x00, 2, 0x00));
        assert!(cartridge.is_cgb());
    }

    #[test]
    fn mbc1_rom_and_ram_banking() {
        let mut cartridge = Cartridge::new(test_rom(0x03, 4, 0x02));

        cartridge.write(0x2000, 2);
        assert_eq!(cartridge.read(0x4000), 2);

        cartridge.write(0x0000, 0x0A);
        cartridge.write(0xA000, 0x55);
        assert_eq!(cartridge.read(0xA000), 0x55);

        cartridge.write(0x0000, 0x00);
        assert_eq!(cartridge.read(0xA000), 0xFF);
    }

    #[test]
    fn mbc1_mode_one_remaps_lower_rom_bank() {
        let mut cartridge = Cartridge::new(test_rom(0x01, 64, 0x00));

        cartridge.write(0x4000, 1);
        cartridge.write(0x6000, 1);

        assert_eq!(cartridge.read(0x0000), 32);
        assert_eq!(cartridge.read(0x4000), 33);
    }

    #[test]
    fn mbc3_rom_and_ram_banking() {
        let mut cartridge = Cartridge::new(test_rom(0x13, 8, 0x02));

        cartridge.write(0x2000, 3);
        assert_eq!(cartridge.read(0x4000), 3);

        cartridge.write(0x0000, 0x0A);
        cartridge.write(0x4000, 1);
        cartridge.write(0xA000, 0x77);
        assert_eq!(cartridge.read(0xA000), 0x77);
    }

    #[test]
    fn mbc3_rtc_registers_latch() {
        let mut cartridge = Cartridge::new(test_rom(0x10, 2, 0x00));

        cartridge.write(0x0000, 0x0A);
        cartridge.write(0x4000, 0x08);
        cartridge.write(0xA000, 42);

        cartridge.write(0x6000, 0);
        cartridge.write(0x6000, 1);

        assert_eq!(cartridge.read(0xA000), 42);
    }

    #[test]
    fn mbc3_without_rtc_keeps_rtc_register_writes_in_ram() {
        let mut cartridge = Cartridge::new(test_rom(0x13, 2, 0x03));

        cartridge.write(0x0000, 0x0A);
        cartridge.write(0x4000, 0x09);
        cartridge.write(0xA000, 0x55);

        assert_eq!(cartridge.read(0xA000), 0x55);
        assert_eq!(cartridge.read(0xA001), 0x00);
    }

    #[test]
    fn mbc5_rom_and_ram_banking() {
        let mut cartridge = Cartridge::new(test_rom(0x1B, 16, 0x03));

        cartridge.write(0x2000, 5);
        assert_eq!(cartridge.read(0x4000), 5);

        cartridge.write(0x3000, 1);
        assert_eq!(cartridge.rom_bank, 0x0105);

        cartridge.write(0x0000, 0x0A);
        cartridge.write(0x4000, 2);
        cartridge.write(0xA000, 0x99);
        assert_eq!(cartridge.read(0xA000), 0x99);
    }

    #[test]
    fn mbc5_maps_ninth_rom_bank_bit() {
        let mut rom = test_rom(0x19, 512, 0x00);
        rom[261 * ROM_BANK_SIZE] = 0xAB;
        let mut cartridge = Cartridge::new(rom);

        cartridge.write(0x2000, 0x05);
        cartridge.write(0x3000, 0x01);

        assert_eq!(cartridge.read(0x4000), 0xAB);
    }

    #[test]
    fn save_ram_round_trips_as_bytes() {
        let mut cartridge = Cartridge::new(test_rom(0x03, 4, 0x02));
        cartridge.set_save_ram(&[0x11, 0x22, 0x33]);
        assert_eq!(&cartridge.save_ram()[..3], &[0x11, 0x22, 0x33]);

        cartridge.set_save_ram(&[0xAA; 8 * 1024]);
        assert_eq!(cartridge.save_ram().len(), 8 * 1024);
        assert!(cartridge.save_ram().iter().all(|&byte| byte == 0xAA));
    }
}
