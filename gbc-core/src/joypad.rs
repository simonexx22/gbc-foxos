//! Joypad input state.
//!
//! The frontend feeds button transitions in through [`Joypad::set_button`].
//! The P1 register exposes the two active-low selection groups used by the
//! hardware.

use crate::state::{SaveStateError, StateReader, StateWriter};
use crate::Button;

/// The eight digital inputs and the P1 selection lines.
#[derive(Debug)]
pub struct Joypad {
    pressed: [bool; Button::COUNT],
    select_directions: bool,
    select_actions: bool,
    interrupt_requested: bool,
}

impl Default for Joypad {
    fn default() -> Self {
        Self {
            pressed: [false; Button::COUNT],
            select_directions: true,
            select_actions: true,
            interrupt_requested: false,
        }
    }
}

impl Joypad {
    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        for b in &self.pressed {
            w.write_bool(*b);
        }
        w.write_bool(self.select_directions);
        w.write_bool(self.select_actions);
        w.write_bool(self.interrupt_requested);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        for b in &mut self.pressed {
            *b = r.read_bool()?;
        }
        self.select_directions = r.read_bool()?;
        self.select_actions = r.read_bool()?;
        self.interrupt_requested = r.read_bool()?;
        Ok(())
    }

    /// Updates the state of one button.
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        let index = button.index();
        let was_pressed = self.pressed[index];
        self.pressed[index] = pressed;

        if pressed && !was_pressed {
            self.interrupt_requested = true;
        }
    }

    /// Returns whether a button is currently held.
    pub fn is_pressed(&self, button: Button) -> bool {
        self.pressed[button.index()]
    }

    /// Reads the P1 register (`FF00`).
    pub fn read(&self) -> u8 {
        let mut value = 0xC0;

        if !self.select_directions {
            value |= 0x10;
        }
        if !self.select_actions {
            value |= 0x20;
        }

        let mut low = 0x0F;

        if self.select_directions {
            low &= self.direction_low();
        }
        if self.select_actions {
            low &= self.action_low();
        }

        value | low
    }

    /// Writes the P1 register (`FF00`).
    pub fn write(&mut self, value: u8) {
        self.select_directions = value & 0x10 == 0;
        self.select_actions = value & 0x20 == 0;
    }

    /// Takes the pending joypad interrupt request, if any.
    pub fn take_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.interrupt_requested)
    }

    fn direction_low(&self) -> u8 {
        let mut low = 0x0F;

        for button in [Button::Right, Button::Left, Button::Up, Button::Down] {
            let bit = match button {
                Button::Right => 0,
                Button::Left => 1,
                Button::Up => 2,
                _ => 3,
            };

            if self.pressed[button.index()] {
                low &= !(1 << bit);
            }
        }

        low
    }

    fn action_low(&self) -> u8 {
        let mut low = 0x0F;

        for button in [Button::A, Button::B, Button::Select, Button::Start] {
            let bit = match button {
                Button::A => 0,
                Button::B => 1,
                Button::Select => 2,
                _ => 3,
            };

            if self.pressed[button.index()] {
                low &= !(1 << bit);
            }
        }

        low
    }
}
