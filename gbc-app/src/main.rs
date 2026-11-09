//! Desktop frontend for the Game Boy Color emulator.
//!
//! Responsibilities kept out of `gbc-core`:
//!
//! * reading the ROM from disk;
//! * creating the window and presenting the RGBA framebuffer;
//! * opening an audio stream and feeding it APU samples;
//! * persisting cartridge save RAM.

mod app;
mod audio;
mod input;
mod save;

use std::path::PathBuf;

use winit::event_loop::EventLoop;

use crate::app::App;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(rom_path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gbc-app <path-to-rom>");
        std::process::exit(2);
    };

    let rom = std::fs::read(&rom_path)?;
    let event_loop = EventLoop::new()?;
    let mut app = App::new(rom_path, rom);

    event_loop.run_app(&mut app)?;
    Ok(())
}
