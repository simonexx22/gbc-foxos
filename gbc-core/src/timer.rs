//! Timer and divider registers.
//!
//! Implements DIV, TIMA, TMA, and TAC with falling-edge TIMA increments based
//! on the 16-bit internal divider, following Pan Docs. TIMA overflows are
//! delayed by one M-cycle (four T-cycles): TIMA reads `0x00` during that
//! window, then is reloaded from TMA and the timer interrupt is requested.
//! Writing TIMA during the delay cancels the reload; writes during the reload
//! cycle are overridden by TMA.

use crate::state::{SaveStateError, StateReader, StateWriter};

const TAC_ENABLE: u8 = 0x04;

/// T-cycles between a TIMA overflow and the TMA reload / interrupt request.
const RELOAD_DELAY: u8 = 4;

/// The DIV/TIMA/TMA/TAC timer block.
#[derive(Debug, Default)]
pub struct Timer {
    /// Internal 16-bit divider. DIV (`FF04`) is the upper byte.
    divider: u16,
    tima: u8,
    tma: u8,
    tac: u8,
    /// T-cycles remaining before a pending TMA reload, or zero when idle.
    reload_delay: u8,
    /// True during the T-cycle in which TMA has just been copied to TIMA.
    reload_cycle: bool,
}

impl Timer {
    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_u16(self.divider);
        w.write_u8(self.tima);
        w.write_u8(self.tma);
        w.write_u8(self.tac);
        w.write_u8(self.reload_delay);
        w.write_bool(self.reload_cycle);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.divider = r.read_u16()?;
        self.tima = r.read_u8()?;
        self.tma = r.read_u8()?;
        self.tac = r.read_u8()?;
        self.reload_delay = r.read_u8()?;
        self.reload_cycle = r.read_bool()?;
        Ok(())
    }

    /// Reads one of the timer registers (`FF04`..=`FF07`).
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.divider >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8,
            _ => 0xFF,
        }
    }

    /// Writes one of the timer registers (`FF04`..=`FF07`).
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF04 => {
                let old = self.selected_bit();
                self.divider = 0;
                self.increment_on_falling_edge(old, self.selected_bit());
            }
            0xFF05 => {
                if self.reload_cycle {
                    // The TMA load wins for the whole reload cycle.
                } else {
                    // A write during the overflow delay cancels the reload and
                    // the interrupt request.
                    self.tima = value;
                    self.reload_delay = 0;
                }
            }
            0xFF06 => {
                self.tma = value;
                if self.reload_cycle {
                    self.tima = value;
                }
            }
            0xFF07 => {
                let old = self.selected_bit();
                self.tac = value & 0x07;
                self.increment_on_falling_edge(old, self.selected_bit());
            }
            _ => {}
        }
    }

    /// Advances the timer by `cycles` T-cycles.
    ///
    /// Returns `true` when a pending overflow completes its reload and the
    /// timer interrupt should be requested.
    pub fn tick(&mut self, cycles: u32) -> bool {
        let mut interrupt = false;

        for _ in 0..cycles {
            interrupt |= self.tick_one();
        }

        interrupt
    }

    fn tick_one(&mut self) -> bool {
        // The reload cycle lasts for one T-cycle.
        self.reload_cycle = false;

        let mut interrupt = false;

        if self.reload_delay > 0 {
            self.reload_delay -= 1;

            if self.reload_delay == 0 {
                self.tima = self.tma;
                self.reload_cycle = true;
                interrupt = true;
            }
        }

        let old = self.selected_bit();
        self.divider = self.divider.wrapping_add(1);
        self.increment_on_falling_edge(old, self.selected_bit());

        interrupt
    }

    fn selected_bit(&self) -> bool {
        if self.tac & TAC_ENABLE == 0 {
            return false;
        }

        let bit = match self.tac & 0x03 {
            0 => 9,
            1 => 3,
            2 => 5,
            _ => 7,
        };

        (self.divider >> bit) & 1 != 0
    }

    fn increment_on_falling_edge(&mut self, old: bool, new: bool) {
        if !old || new {
            return;
        }

        let (value, overflow) = self.tima.overflowing_add(1);

        if overflow {
            self.tima = 0;
            self.reload_delay = RELOAD_DELAY;
            self.reload_cycle = false;
        } else {
            self.tima = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer_with(tac: u8, tima: u8, tma: u8) -> Timer {
        let mut timer = Timer::default();
        timer.write(0xFF06, tma);
        timer.write(0xFF05, tima);
        timer.write(0xFF07, tac);
        timer
    }

    #[test]
    fn div_increments_every_256_t_cycles() {
        let mut timer = Timer::default();

        timer.tick(255);
        assert_eq!(timer.read(0xFF04), 0);

        timer.tick(1);
        assert_eq!(timer.read(0xFF04), 1);
    }

    #[test]
    fn tima_clock_selects_the_correct_divider_bit() {
        for (tac, period) in [(0x04, 1024), (0x05, 16), (0x06, 64), (0x07, 256)] {
            let mut timer = timer_with(tac, 0, 0);

            timer.tick(period - 1);
            assert_eq!(timer.read(0xFF05), 0, "TAC {tac:#04x}");

            timer.tick(1);
            assert_eq!(timer.read(0xFF05), 1, "TAC {tac:#04x}");
        }
    }

    #[test]
    fn div_write_can_increment_tima_on_a_falling_edge() {
        let mut timer = timer_with(0x05, 0, 0);

        timer.tick(8);
        timer.write(0xFF04, 0);

        assert_eq!(timer.read(0xFF05), 1);
        assert_eq!(timer.read(0xFF04), 0);
    }

    #[test]
    fn tac_write_can_increment_tima_on_a_falling_edge() {
        let mut timer = timer_with(0x05, 0, 0);

        timer.tick(8);
        timer.write(0xFF07, 0x00);

        assert_eq!(timer.read(0xFF05), 1);
    }

    #[test]
    fn overflow_waits_four_cycles_before_reloading() {
        let mut timer = timer_with(0x06, 0xFF, 0x42);

        timer.tick(63);
        assert_eq!(timer.read(0xFF05), 0xFF);
        assert!(!timer.tick(1));
        assert_eq!(timer.read(0xFF05), 0x00);

        for _ in 0..3 {
            assert!(!timer.tick(1));
            assert_eq!(timer.read(0xFF05), 0x00);
        }

        assert!(timer.tick(1));
        assert_eq!(timer.read(0xFF05), 0x42);
    }

    #[test]
    fn tima_write_during_overflow_delay_cancels_reload() {
        let mut timer = timer_with(0x06, 0xFF, 0x42);

        timer.tick(64);
        assert_eq!(timer.read(0xFF05), 0x00);

        timer.write(0xFF05, 0x7F);
        assert_eq!(timer.read(0xFF05), 0x7F);

        assert!(!timer.tick(64));
        assert_eq!(timer.read(0xFF05), 0x80);

        assert!(!timer.tick(4));
        assert_eq!(timer.read(0xFF05), 0x80);
    }

    #[test]
    fn tima_write_during_reload_cycle_is_ignored() {
        let mut timer = timer_with(0x06, 0xFF, 0x42);

        timer.tick(64);
        assert!(timer.tick(4));
        assert_eq!(timer.read(0xFF05), 0x42);

        timer.write(0xFF05, 0x7F);
        assert_eq!(timer.read(0xFF05), 0x42);
    }

    #[test]
    fn tma_write_during_reload_cycle_updates_tima() {
        let mut timer = timer_with(0x06, 0xFF, 0x42);

        timer.tick(64);
        assert!(timer.tick(4));

        timer.write(0xFF06, 0x7F);
        assert_eq!(timer.read(0xFF05), 0x7F);
        assert_eq!(timer.read(0xFF06), 0x7F);
    }
}
