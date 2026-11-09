//! Game Boy buttons.
//!
//! The core only models the eight digital inputs; mapping physical keys to
//! these values is the frontend's responsibility.

/// A Game Boy button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    Right = 0,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

impl Button {
    /// All buttons, in a stable order.
    pub const ALL: [Self; 8] = [
        Self::Right,
        Self::Left,
        Self::Up,
        Self::Down,
        Self::A,
        Self::B,
        Self::Select,
        Self::Start,
    ];

    /// Number of buttons exposed by the hardware.
    pub const COUNT: usize = Self::ALL.len();

    /// Index into a fixed-size button state array.
    pub const fn index(self) -> usize {
        self as usize
    }
}
