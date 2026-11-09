//! Serial port.
//!
//! The test harness uses the serial link as a byte stream. Writes to `SC` that
//! start an internal-clock transfer immediately shift `SB` out and append it
//! to the captured output buffer.

use crate::state::{SaveStateError, StateReader, StateWriter};

/// Serial transfer state and captured output.
#[derive(Debug, Default)]
pub struct Serial {
    sb: u8,
    sc: u8,
    output: Vec<u8>,
}

impl Serial {
    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_u8(self.sb);
        w.write_u8(self.sc);
        w.write_bytes(&self.output);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.sb = r.read_u8()?;
        self.sc = r.read_u8()?;
        let out = r.read_bytes()?;
        self.output = out.to_vec();
        Ok(())
    }

    /// Reads the serial data register (`FF01`).
    pub fn read_sb(&self) -> u8 {
        self.sb
    }

    /// Writes the serial data register (`FF01`).
    pub fn write_sb(&mut self, value: u8) {
        self.sb = value;
    }

    /// Reads the serial control register (`FF02`).
    ///
    /// Transfers complete immediately in this implementation, so the busy bit
    /// is never visible to software.
    pub fn read_sc(&self) -> u8 {
        self.sc
    }

    /// Writes the serial control register (`FF02`).
    ///
    /// Returns `true` when a new byte was shifted out and should raise a
    /// serial interrupt.
    pub fn write_sc(&mut self, value: u8) -> bool {
        self.sc = value & 0x7F;

        if value & 0x80 == 0 {
            return false;
        }

        // Only internal-clock transfers produce output for the test runner.
        if value & 0x01 != 0 {
            self.output.push(self.sb);
            self.sc &= !0x80;
            return true;
        }

        self.sc &= !0x80;
        false
    }

    /// All bytes captured so far.
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Removes and returns all bytes captured so far.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Clears the captured output buffer.
    pub fn clear_output(&mut self) {
        self.output.clear();
    }
}
