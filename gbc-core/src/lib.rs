//! Platform-independent Game Boy Color emulation core.
//!
//! This crate deliberately has no platform dependencies. It performs no
//! windowing, no audio output, and no filesystem access. The frontend is
//! responsible for:
//!
//! * reading ROMs and passing them to [`Gameboy::new`];
//! * presenting [`Gameboy::framebuffer`] (RGBA8888);
//! * playing [`Gameboy::audio_samples`] (interleaved stereo `f32`);
//! * persisting [`Gameboy::save_ram`] and restoring it with
//!   [`Gameboy::set_save_ram`].
//!
//! All emulated components are owned directly by [`Gameboy`]. Components never
//! store references to one another; the top-level [`Gameboy`] coordinates
//! them.

#![forbid(unsafe_code)]

pub mod apu;
pub mod bus;
pub mod button;
pub mod cartridge;
pub mod cpu;
pub mod gameboy;
pub mod joypad;
pub mod mmu;
pub mod ppu;
pub mod serial;
pub mod state;
pub mod timer;

pub use button::Button;
pub use gameboy::Gameboy;
pub use state::SaveStateError;

/// Width of the Game Boy Color LCD, in pixels.
pub const SCREEN_WIDTH: usize = 160;

/// Height of the Game Boy Color LCD, in pixels.
pub const SCREEN_HEIGHT: usize = 144;

/// Number of bytes in an RGBA8888 frame (`160 * 144 * 4`).
pub const FRAMEBUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

/// Preferred output sample rate for the audio frontend.
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;

/// Number of interleaved audio channels produced by the APU.
pub const AUDIO_CHANNELS: usize = 2;
