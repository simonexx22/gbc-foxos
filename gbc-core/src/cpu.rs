//! Sharp LR35902 CPU.
//!
//! Implements the complete SM83 instruction set, the standard flag behaviour,
//! CB-prefixed instructions, interrupt dispatch, the EI delay, HALT, and the
//! HALT bug. Cycle counts follow Pan Docs.

use crate::bus::Bus;
use crate::state::{SaveStateError, StateReader, StateWriter};

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;
const FLAG_C: u8 = 0x10;

const INTERRUPT_ENABLE: u16 = 0xFFFF;
const INTERRUPT_FLAGS: u16 = 0xFF0F;

/// SM83 register file.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
}

/// The processor and its architectural state.
#[derive(Debug)]
pub struct Cpu {
    registers: Registers,
    pc: u16,
    sp: u16,
    ime: bool,
    ime_delay: u8,
    halted: bool,
    halt_bug: bool,
    stopped: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    /// Creates a CPU in the standard DMG post-boot state.
    pub fn new() -> Self {
        Self {
            registers: Registers {
                a: 0x01,
                f: 0xB0,
                b: 0x00,
                c: 0x13,
                d: 0x00,
                e: 0xD8,
                h: 0x01,
                l: 0x4D,
            },
            pc: 0x0100,
            sp: 0xFFFE,
            ime: false,
            ime_delay: 0,
            halted: false,
            halt_bug: false,
            stopped: false,
        }
    }

    /// Creates a CPU in the post-boot state for the detected console.
    ///
    /// CGB software detects color hardware by reading `A == 0x11` after boot.
    pub fn for_system(cgb: bool) -> Self {
        let mut cpu = Self::new();

        if cgb {
            cpu.registers.a = 0x11;
        }

        cpu
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_u8(self.registers.a);
        w.write_u8(self.registers.f);
        w.write_u8(self.registers.b);
        w.write_u8(self.registers.c);
        w.write_u8(self.registers.d);
        w.write_u8(self.registers.e);
        w.write_u8(self.registers.h);
        w.write_u8(self.registers.l);
        w.write_u16(self.pc);
        w.write_u16(self.sp);
        w.write_bool(self.ime);
        w.write_u8(self.ime_delay);
        w.write_bool(self.halted);
        w.write_bool(self.halt_bug);
        w.write_bool(self.stopped);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.registers.a = r.read_u8()?;
        self.registers.f = r.read_u8()?;
        self.registers.b = r.read_u8()?;
        self.registers.c = r.read_u8()?;
        self.registers.d = r.read_u8()?;
        self.registers.e = r.read_u8()?;
        self.registers.h = r.read_u8()?;
        self.registers.l = r.read_u8()?;
        self.pc = r.read_u16()?;
        self.sp = r.read_u16()?;
        self.ime = r.read_bool()?;
        self.ime_delay = r.read_u8()?;
        self.halted = r.read_bool()?;
        self.halt_bug = r.read_bool()?;
        self.stopped = r.read_bool()?;
        Ok(())
    }

    /// Executes one instruction or interrupt dispatch and returns the number
    /// of elapsed T-cycles.
    pub fn step<B: Bus + ?Sized>(&mut self, bus: &mut B) -> u32 {
        if self.stopped {
            let pending = bus.read(INTERRUPT_ENABLE) & bus.read(INTERRUPT_FLAGS) & 0x1F;
            if pending != 0 {
                self.stopped = false;
                return 4;
            }
            return 4;
        }

        if self.halted {
            let pending = bus.read(INTERRUPT_ENABLE) & bus.read(INTERRUPT_FLAGS) & 0x1F;
            if pending == 0 {
                return 4;
            }

            self.halted = false;
            return 4;
        }

        if self.ime {
            let pending = bus.read(INTERRUPT_ENABLE) & bus.read(INTERRUPT_FLAGS) & 0x1F;
            if pending != 0 {
                return self.service_interrupt(bus, pending);
            }
        }

        let opcode = self.fetch_opcode(bus);
        let cycles = self.execute(opcode, bus);

        if self.ime_delay > 0 {
            self.ime_delay -= 1;
            if self.ime_delay == 0 {
                self.ime = true;
            }
        }

        cycles
    }

    fn service_interrupt<B: Bus + ?Sized>(&mut self, bus: &mut B, pending: u8) -> u32 {
        let bit = pending.trailing_zeros() as u8;
        let flags = bus.read(INTERRUPT_FLAGS) & !(1 << bit);
        bus.write(INTERRUPT_FLAGS, flags);

        self.ime = false;
        self.ime_delay = 0;
        self.halted = false;
        self.push16(bus, self.pc);
        self.pc = 0x0040 + (bit as u16) * 8;

        20
    }

    fn fetch_opcode<B: Bus + ?Sized>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.pc);

        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }

        value
    }

    fn fetch8<B: Bus + ?Sized>(&mut self, bus: &mut B) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn fetch16<B: Bus + ?Sized>(&mut self, bus: &mut B) -> u16 {
        let low = self.fetch8(bus) as u16;
        let high = self.fetch8(bus) as u16;
        low | (high << 8)
    }

    fn write16<B: Bus + ?Sized>(&self, bus: &mut B, addr: u16, value: u16) {
        bus.write(addr, value as u8);
        bus.write(addr.wrapping_add(1), (value >> 8) as u8);
    }

    fn push16<B: Bus + ?Sized>(&mut self, bus: &mut B, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, value as u8);
    }

    fn pop16<B: Bus + ?Sized>(&mut self, bus: &mut B) -> u16 {
        let low = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let high = bus.read(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        low | (high << 8)
    }

    fn read_reg<B: Bus + ?Sized>(&self, index: u8, bus: &mut B) -> u8 {
        if index == 6 {
            bus.read(self.hl())
        } else {
            self.register(index)
        }
    }

    fn write_reg<B: Bus + ?Sized>(&mut self, index: u8, value: u8, bus: &mut B) {
        if index == 6 {
            bus.write(self.hl(), value);
        } else {
            self.set_register(index, value);
        }
    }

    fn register(&self, index: u8) -> u8 {
        match index {
            0 => self.registers.b,
            1 => self.registers.c,
            2 => self.registers.d,
            3 => self.registers.e,
            4 => self.registers.h,
            5 => self.registers.l,
            7 => self.registers.a,
            _ => 0,
        }
    }

    fn set_register(&mut self, index: u8, value: u8) {
        match index {
            0 => self.registers.b = value,
            1 => self.registers.c = value,
            2 => self.registers.d = value,
            3 => self.registers.e = value,
            4 => self.registers.h = value,
            5 => self.registers.l = value,
            7 => self.registers.a = value,
            _ => {}
        }
    }

    fn a(&self) -> u8 {
        self.registers.a
    }

    fn set_a(&mut self, value: u8) {
        self.registers.a = value;
    }

    fn f(&self) -> u8 {
        self.registers.f
    }

    fn set_f(&mut self, value: u8) {
        self.registers.f = value & 0xF0;
    }

    fn bc(&self) -> u16 {
        u16::from_le_bytes([self.registers.c, self.registers.b])
    }

    fn set_bc(&mut self, value: u16) {
        self.registers.c = value as u8;
        self.registers.b = (value >> 8) as u8;
    }

    fn de(&self) -> u16 {
        u16::from_le_bytes([self.registers.e, self.registers.d])
    }

    fn set_de(&mut self, value: u16) {
        self.registers.e = value as u8;
        self.registers.d = (value >> 8) as u8;
    }

    fn hl(&self) -> u16 {
        u16::from_le_bytes([self.registers.l, self.registers.h])
    }

    fn set_hl(&mut self, value: u16) {
        self.registers.l = value as u8;
        self.registers.h = (value >> 8) as u8;
    }

    fn af(&self) -> u16 {
        u16::from_le_bytes([self.f(), self.a()])
    }

    fn set_af(&mut self, value: u16) {
        self.set_a((value >> 8) as u8);
        self.set_f(value as u8);
    }

    fn flag(&self, flag: u8) -> bool {
        self.f() & flag != 0
    }

    fn set_flag(&mut self, flag: u8, set: bool) {
        if set {
            self.registers.f |= flag;
        } else {
            self.registers.f &= !flag;
        }
    }

    fn inc8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_add(1);
        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, value & 0x0F == 0x0F);
        result
    }

    fn dec8(&mut self, value: u8) -> u8 {
        let result = value.wrapping_sub(1);
        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(FLAG_H, value & 0x0F == 0);
        result
    }

    fn add_a(&mut self, value: u8, carry: bool) -> u8 {
        let a = self.a();
        let carry_in = if carry && self.flag(FLAG_C) { 1 } else { 0 };
        let result = a as u16 + value as u16 + carry_in;

        self.set_flag(FLAG_Z, result as u8 == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(
            FLAG_H,
            (a & 0x0F) as u16 + (value & 0x0F) as u16 + carry_in > 0x0F,
        );
        self.set_flag(FLAG_C, result > 0xFF);

        result as u8
    }

    fn sub_a(&mut self, value: u8, carry: bool) -> u8 {
        let a = self.a();
        let carry_in = if carry && self.flag(FLAG_C) { 1 } else { 0 };
        let result = (a as u16).wrapping_sub(value as u16 + carry_in);

        self.set_flag(FLAG_Z, result as u8 == 0);
        self.set_flag(FLAG_N, true);
        self.set_flag(
            FLAG_H,
            ((a & 0x0F) as u16) < (value & 0x0F) as u16 + carry_in,
        );
        self.set_flag(FLAG_C, (a as u16) < value as u16 + carry_in);

        result as u8
    }

    fn alu(&mut self, operation: u8, value: u8) {
        match operation {
            0 => {
                let result = self.add_a(value, false);
                self.set_a(result);
            }
            1 => {
                let result = self.add_a(value, true);
                self.set_a(result);
            }
            2 => {
                let result = self.sub_a(value, false);
                self.set_a(result);
            }
            3 => {
                let result = self.sub_a(value, true);
                self.set_a(result);
            }
            4 => {
                let result = self.a() & value;
                self.set_a(result);
                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, true);
                self.set_flag(FLAG_C, false);
            }
            5 => {
                let result = self.a() ^ value;
                self.set_a(result);
                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, false);
            }
            6 => {
                let result = self.a() | value;
                self.set_a(result);
                self.set_flag(FLAG_Z, result == 0);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, false);
            }
            _ => {
                let _ = self.sub_a(value, false);
            }
        }
    }

    fn add_hl(&mut self, value: u16) {
        let hl = self.hl();
        let result = hl as u32 + value as u32;

        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (hl & 0x0FFF) + (value & 0x0FFF) > 0x0FFF);
        self.set_flag(FLAG_C, result > 0xFFFF);
        self.set_hl(result as u16);
    }

    fn add_sp_offset(&mut self, offset: u8) -> u16 {
        let sp = self.sp;
        let signed = offset as i8 as i16;
        let result = sp.wrapping_add_signed(signed);

        self.set_flag(FLAG_Z, false);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, (sp & 0x0F) + (offset as u16 & 0x0F) > 0x0F);
        self.set_flag(FLAG_C, (sp & 0xFF) + (offset as u16 & 0xFF) > 0xFF);

        result
    }

    fn daa(&mut self) {
        let mut value = self.a();
        let subtract = self.flag(FLAG_N);
        let mut carry = self.flag(FLAG_C);
        let mut correction = 0;

        if self.flag(FLAG_H) || (!subtract && (value & 0x0F) > 9) {
            correction |= 0x06;
        }
        if carry || (!subtract && value > 0x99) {
            correction |= 0x60;
            carry = true;
        }

        if subtract {
            value = value.wrapping_sub(correction);
        } else {
            value = value.wrapping_add(correction);
        }

        self.set_a(value);
        self.set_flag(FLAG_Z, value == 0);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
    }

    fn rlc(&mut self, value: u8) -> u8 {
        let carry = value & 0x80 != 0;
        let result = (value << 1) | u8::from(carry);

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn rrc(&mut self, value: u8) -> u8 {
        let carry = value & 0x01 != 0;
        let result = (value >> 1) | (u8::from(carry) << 7);

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn rl(&mut self, value: u8) -> u8 {
        let carry = value & 0x80 != 0;
        let result = (value << 1) | u8::from(self.flag(FLAG_C));

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn rr(&mut self, value: u8) -> u8 {
        let carry = value & 0x01 != 0;
        let result = (value >> 1) | (u8::from(self.flag(FLAG_C)) << 7);

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn sla(&mut self, value: u8) -> u8 {
        let carry = value & 0x80 != 0;
        let result = value << 1;

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn sra(&mut self, value: u8) -> u8 {
        let carry = value & 0x01 != 0;
        let result = (value >> 1) | (value & 0x80);

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn swap(&mut self, value: u8) -> u8 {
        let result = value.rotate_right(4);

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, false);
        result
    }

    fn srl(&mut self, value: u8) -> u8 {
        let carry = value & 0x01 != 0;
        let result = value >> 1;

        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, false);
        self.set_flag(FLAG_C, carry);
        result
    }

    fn bit(&mut self, bit: u8, value: u8) {
        self.set_flag(FLAG_Z, value & (1 << bit) == 0);
        self.set_flag(FLAG_N, false);
        self.set_flag(FLAG_H, true);
    }

    fn execute_cb<B: Bus + ?Sized>(&mut self, opcode: u8, bus: &mut B) -> u32 {
        let register = opcode & 0x07;
        let bit = (opcode >> 3) & 0x07;
        let group = opcode >> 6;

        match group {
            0 => {
                let value = self.read_reg(register, bus);
                let result = match bit {
                    0 => self.rlc(value),
                    1 => self.rrc(value),
                    2 => self.rl(value),
                    3 => self.rr(value),
                    4 => self.sla(value),
                    5 => self.sra(value),
                    6 => self.swap(value),
                    _ => self.srl(value),
                };
                self.write_reg(register, result, bus);

                if register == 6 {
                    16
                } else {
                    8
                }
            }
            1 => {
                let value = self.read_reg(register, bus);
                self.bit(bit, value);

                if register == 6 {
                    12
                } else {
                    8
                }
            }
            2 => {
                let value = self.read_reg(register, bus);
                self.write_reg(register, value & !(1 << bit), bus);

                if register == 6 {
                    16
                } else {
                    8
                }
            }
            _ => {
                let value = self.read_reg(register, bus);
                self.write_reg(register, value | (1 << bit), bus);

                if register == 6 {
                    16
                } else {
                    8
                }
            }
        }
    }

    fn execute<B: Bus + ?Sized>(&mut self, opcode: u8, bus: &mut B) -> u32 {
        match opcode {
            0x00 => 4,

            // 0x01..=0x3F
            0x01 => {
                let value = self.fetch16(bus);
                self.set_bc(value);
                12
            }
            0x02 => {
                bus.write(self.bc(), self.a());
                8
            }
            0x03 => {
                self.set_bc(self.bc().wrapping_add(1));
                8
            }
            0x04 => {
                let value = self.inc8(self.registers.b);
                self.registers.b = value;
                4
            }
            0x05 => {
                let value = self.dec8(self.registers.b);
                self.registers.b = value;
                4
            }
            0x06 => {
                let value = self.fetch8(bus);
                self.registers.b = value;
                8
            }
            0x07 => {
                let a = self.a();
                let carry = a & 0x80 != 0;
                let value = (a << 1) | u8::from(carry);
                self.set_a(value);
                self.set_flag(FLAG_Z, false);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                4
            }
            0x08 => {
                let addr = self.fetch16(bus);
                self.write16(bus, addr, self.sp);
                20
            }
            0x09 => {
                let value = self.bc();
                self.add_hl(value);
                8
            }
            0x0A => {
                let value = bus.read(self.bc());
                self.set_a(value);
                8
            }
            0x0B => {
                self.set_bc(self.bc().wrapping_sub(1));
                8
            }
            0x0C => {
                let value = self.inc8(self.registers.c);
                self.registers.c = value;
                4
            }
            0x0D => {
                let value = self.dec8(self.registers.c);
                self.registers.c = value;
                4
            }
            0x0E => {
                let value = self.fetch8(bus);
                self.registers.c = value;
                8
            }
            0x0F => {
                let a = self.a();
                let carry = a & 0x01 != 0;
                let value = (a >> 1) | (u8::from(carry) << 7);
                self.set_a(value);
                self.set_flag(FLAG_Z, false);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                4
            }
            0x10 => {
                let _ = self.fetch8(bus);
                if !bus.speed_switch() {
                    self.stopped = true;
                }
                4
            }
            0x11 => {
                let value = self.fetch16(bus);
                self.set_de(value);
                12
            }
            0x12 => {
                bus.write(self.de(), self.a());
                8
            }
            0x13 => {
                self.set_de(self.de().wrapping_add(1));
                8
            }
            0x14 => {
                let value = self.inc8(self.registers.d);
                self.registers.d = value;
                4
            }
            0x15 => {
                let value = self.dec8(self.registers.d);
                self.registers.d = value;
                4
            }
            0x16 => {
                let value = self.fetch8(bus);
                self.registers.d = value;
                8
            }
            0x17 => {
                let a = self.a();
                let carry = a & 0x80 != 0;
                let value = (a << 1) | u8::from(self.flag(FLAG_C));
                self.set_a(value);
                self.set_flag(FLAG_Z, false);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                4
            }
            0x18 => {
                let offset = self.fetch8(bus) as i8 as i16;
                self.pc = self.pc.wrapping_add_signed(offset);
                12
            }
            0x19 => {
                let value = self.de();
                self.add_hl(value);
                8
            }
            0x1A => {
                let value = bus.read(self.de());
                self.set_a(value);
                8
            }
            0x1B => {
                self.set_de(self.de().wrapping_sub(1));
                8
            }
            0x1C => {
                let value = self.inc8(self.registers.e);
                self.registers.e = value;
                4
            }
            0x1D => {
                let value = self.dec8(self.registers.e);
                self.registers.e = value;
                4
            }
            0x1E => {
                let value = self.fetch8(bus);
                self.registers.e = value;
                8
            }
            0x1F => {
                let a = self.a();
                let carry = a & 0x01 != 0;
                let value = (a >> 1) | (u8::from(self.flag(FLAG_C)) << 7);
                self.set_a(value);
                self.set_flag(FLAG_Z, false);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                4
            }
            0x20 => {
                let offset = self.fetch8(bus) as i8 as i16;
                if !self.flag(FLAG_Z) {
                    self.pc = self.pc.wrapping_add_signed(offset);
                    12
                } else {
                    8
                }
            }
            0x21 => {
                let value = self.fetch16(bus);
                self.set_hl(value);
                12
            }
            0x22 => {
                let addr = self.hl();
                bus.write(addr, self.a());
                self.set_hl(addr.wrapping_add(1));
                8
            }
            0x23 => {
                self.set_hl(self.hl().wrapping_add(1));
                8
            }
            0x24 => {
                let value = self.inc8(self.registers.h);
                self.registers.h = value;
                4
            }
            0x25 => {
                let value = self.dec8(self.registers.h);
                self.registers.h = value;
                4
            }
            0x26 => {
                let value = self.fetch8(bus);
                self.registers.h = value;
                8
            }
            0x27 => {
                self.daa();
                4
            }
            0x28 => {
                let offset = self.fetch8(bus) as i8 as i16;
                if self.flag(FLAG_Z) {
                    self.pc = self.pc.wrapping_add_signed(offset);
                    12
                } else {
                    8
                }
            }
            0x29 => {
                let value = self.hl();
                self.add_hl(value);
                8
            }
            0x2A => {
                let addr = self.hl();
                let value = bus.read(addr);
                self.set_a(value);
                self.set_hl(addr.wrapping_add(1));
                8
            }
            0x2B => {
                self.set_hl(self.hl().wrapping_sub(1));
                8
            }
            0x2C => {
                let value = self.inc8(self.registers.l);
                self.registers.l = value;
                4
            }
            0x2D => {
                let value = self.dec8(self.registers.l);
                self.registers.l = value;
                4
            }
            0x2E => {
                let value = self.fetch8(bus);
                self.registers.l = value;
                8
            }
            0x2F => {
                let value = !self.a();
                self.set_a(value);
                self.set_flag(FLAG_N, true);
                self.set_flag(FLAG_H, true);
                4
            }
            0x30 => {
                let offset = self.fetch8(bus) as i8 as i16;
                if !self.flag(FLAG_C) {
                    self.pc = self.pc.wrapping_add_signed(offset);
                    12
                } else {
                    8
                }
            }
            0x31 => {
                let value = self.fetch16(bus);
                self.sp = value;
                12
            }
            0x32 => {
                let addr = self.hl();
                bus.write(addr, self.a());
                self.set_hl(addr.wrapping_sub(1));
                8
            }
            0x33 => {
                self.sp = self.sp.wrapping_add(1);
                8
            }
            0x34 => {
                let addr = self.hl();
                let value = bus.read(addr);
                let result = self.inc8(value);
                bus.write(addr, result);
                12
            }
            0x35 => {
                let addr = self.hl();
                let value = bus.read(addr);
                let result = self.dec8(value);
                bus.write(addr, result);
                12
            }
            0x36 => {
                let value = self.fetch8(bus);
                bus.write(self.hl(), value);
                12
            }
            0x37 => {
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, true);
                4
            }
            0x38 => {
                let offset = self.fetch8(bus) as i8 as i16;
                if self.flag(FLAG_C) {
                    self.pc = self.pc.wrapping_add_signed(offset);
                    12
                } else {
                    8
                }
            }
            0x39 => {
                let value = self.sp;
                self.add_hl(value);
                8
            }
            0x3A => {
                let addr = self.hl();
                let value = bus.read(addr);
                self.set_a(value);
                self.set_hl(addr.wrapping_sub(1));
                8
            }
            0x3B => {
                self.sp = self.sp.wrapping_sub(1);
                8
            }
            0x3C => {
                let value = self.inc8(self.a());
                self.set_a(value);
                4
            }
            0x3D => {
                let value = self.dec8(self.a());
                self.set_a(value);
                4
            }
            0x3E => {
                let value = self.fetch8(bus);
                self.set_a(value);
                8
            }
            0x3F => {
                let carry = !self.flag(FLAG_C);
                self.set_flag(FLAG_N, false);
                self.set_flag(FLAG_H, false);
                self.set_flag(FLAG_C, carry);
                4
            }

            // LD r, r' and HALT.
            0x40..=0x7F => {
                if opcode == 0x76 {
                    let pending = bus.read(INTERRUPT_ENABLE) & bus.read(INTERRUPT_FLAGS) & 0x1F;

                    if !self.ime && pending != 0 {
                        self.halt_bug = true;
                    } else {
                        self.halted = true;
                    }

                    4
                } else {
                    let destination = (opcode >> 3) & 0x07;
                    let source = opcode & 0x07;
                    let value = self.read_reg(source, bus);
                    self.write_reg(destination, value, bus);

                    if destination == 6 || source == 6 {
                        8
                    } else {
                        4
                    }
                }
            }

            // ALU A, r.
            0x80..=0xBF => {
                let operation = (opcode >> 3) & 0x07;
                let source = opcode & 0x07;
                let value = self.read_reg(source, bus);
                self.alu(operation, value);

                if source == 6 {
                    8
                } else {
                    4
                }
            }

            0xC0 => {
                if !self.flag(FLAG_Z) {
                    self.pc = self.pop16(bus);
                    20
                } else {
                    8
                }
            }
            0xC1 => {
                let value = self.pop16(bus);
                self.set_bc(value);
                12
            }
            0xC2 => {
                let addr = self.fetch16(bus);
                if !self.flag(FLAG_Z) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xC3 => {
                self.pc = self.fetch16(bus);
                16
            }
            0xC4 => {
                let addr = self.fetch16(bus);
                if !self.flag(FLAG_Z) {
                    self.push16(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xC5 => {
                let value = self.bc();
                self.push16(bus, value);
                16
            }
            0xC6 => {
                let value = self.fetch8(bus);
                let result = self.add_a(value, false);
                self.set_a(result);
                8
            }
            0xC7 => {
                self.push16(bus, self.pc);
                self.pc = 0x0000;
                16
            }
            0xC8 => {
                if self.flag(FLAG_Z) {
                    self.pc = self.pop16(bus);
                    20
                } else {
                    8
                }
            }
            0xC9 => {
                self.pc = self.pop16(bus);
                16
            }
            0xCA => {
                let addr = self.fetch16(bus);
                if self.flag(FLAG_Z) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xCB => {
                let cb = self.fetch8(bus);
                self.execute_cb(cb, bus)
            }
            0xCC => {
                let addr = self.fetch16(bus);
                if self.flag(FLAG_Z) {
                    self.push16(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xCD => {
                let addr = self.fetch16(bus);
                self.push16(bus, self.pc);
                self.pc = addr;
                24
            }
            0xCE => {
                let value = self.fetch8(bus);
                let result = self.add_a(value, true);
                self.set_a(result);
                8
            }
            0xCF => {
                self.push16(bus, self.pc);
                self.pc = 0x0008;
                16
            }
            0xD0 => {
                if !self.flag(FLAG_C) {
                    self.pc = self.pop16(bus);
                    20
                } else {
                    8
                }
            }
            0xD1 => {
                let value = self.pop16(bus);
                self.set_de(value);
                12
            }
            0xD2 => {
                let addr = self.fetch16(bus);
                if !self.flag(FLAG_C) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xD3 => 4,
            0xD4 => {
                let addr = self.fetch16(bus);
                if !self.flag(FLAG_C) {
                    self.push16(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xD5 => {
                let value = self.de();
                self.push16(bus, value);
                16
            }
            0xD6 => {
                let value = self.fetch8(bus);
                let result = self.sub_a(value, false);
                self.set_a(result);
                8
            }
            0xD7 => {
                self.push16(bus, self.pc);
                self.pc = 0x0010;
                16
            }
            0xD8 => {
                if self.flag(FLAG_C) {
                    self.pc = self.pop16(bus);
                    20
                } else {
                    8
                }
            }
            0xD9 => {
                self.pc = self.pop16(bus);
                self.ime = true;
                self.ime_delay = 0;
                16
            }
            0xDA => {
                let addr = self.fetch16(bus);
                if self.flag(FLAG_C) {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xDB => 4,
            0xDC => {
                let addr = self.fetch16(bus);
                if self.flag(FLAG_C) {
                    self.push16(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xDD => 4,
            0xDE => {
                let value = self.fetch8(bus);
                let result = self.sub_a(value, true);
                self.set_a(result);
                8
            }
            0xDF => {
                self.push16(bus, self.pc);
                self.pc = 0x0018;
                16
            }
            0xE0 => {
                let offset = self.fetch8(bus) as u16;
                bus.write(0xFF00 + offset, self.a());
                12
            }
            0xE1 => {
                let value = self.pop16(bus);
                self.set_hl(value);
                12
            }
            0xE2 => {
                let addr = 0xFF00 + self.registers.c as u16;
                bus.write(addr, self.a());
                8
            }
            0xE3 => 4,
            0xE4 => 4,
            0xE5 => {
                let value = self.hl();
                self.push16(bus, value);
                16
            }
            0xE6 => {
                let value = self.fetch8(bus);
                self.alu(4, value);
                8
            }
            0xE7 => {
                self.push16(bus, self.pc);
                self.pc = 0x0020;
                16
            }
            0xE8 => {
                let offset = self.fetch8(bus);
                self.sp = self.add_sp_offset(offset);
                16
            }
            0xE9 => {
                self.pc = self.hl();
                4
            }
            0xEA => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.a());
                16
            }
            0xEB => 4,
            0xEC => 4,
            0xED => 4,
            0xEE => {
                let value = self.fetch8(bus);
                self.alu(5, value);
                8
            }
            0xEF => {
                self.push16(bus, self.pc);
                self.pc = 0x0028;
                16
            }
            0xF0 => {
                let offset = self.fetch8(bus) as u16;
                let value = bus.read(0xFF00 + offset);
                self.set_a(value);
                12
            }
            0xF1 => {
                let value = self.pop16(bus);
                self.set_af(value);
                12
            }
            0xF2 => {
                let addr = 0xFF00 + self.registers.c as u16;
                let value = bus.read(addr);
                self.set_a(value);
                8
            }
            0xF3 => {
                self.ime = false;
                self.ime_delay = 0;
                4
            }
            0xF4 => 4,
            0xF5 => {
                let value = self.af();
                self.push16(bus, value);
                16
            }
            0xF6 => {
                let value = self.fetch8(bus);
                self.alu(6, value);
                8
            }
            0xF7 => {
                self.push16(bus, self.pc);
                self.pc = 0x0030;
                16
            }
            0xF8 => {
                let offset = self.fetch8(bus);
                let value = self.add_sp_offset(offset);
                self.set_hl(value);
                12
            }
            0xF9 => {
                self.sp = self.hl();
                8
            }
            0xFA => {
                let addr = self.fetch16(bus);
                let value = bus.read(addr);
                self.set_a(value);
                16
            }
            0xFB => {
                // The first EI schedules the one-instruction delay. Repeated
                // EI instructions must not keep postponing it.
                if self.ime_delay == 0 {
                    self.ime_delay = 2;
                }
                4
            }
            0xFC => 4,
            0xFD => 4,
            0xFE => {
                let value = self.fetch8(bus);
                let _ = self.sub_a(value, false);
                8
            }
            0xFF => {
                self.push16(bus, self.pc);
                self.pc = 0x0038;
                16
            }
        }
    }
}
