#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop frontend for the Game Boy Color emulator and launcher.
//!
//! Responsibilities kept out of `gbc-core`:
//!
//! * reading the ROM from disk;
//! * presenting the launcher or emulator window;
//! * opening an audio stream and feeding it APU samples;
//! * persisting cartridge save RAM, settings, and key mappings.

mod app;
mod audio;
pub mod config;
mod font;
mod input;
pub mod launcher;
mod save;
pub mod strings;

use std::path::PathBuf;

use winit::event_loop::EventLoop;

use crate::app::App;
use crate::config::Config;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os();
    let _bin_name = args.next();

    if let Some(rom_arg) = args.next() {
        let rom_path = PathBuf::from(rom_arg);
        let rom = match std::fs::read(&rom_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!(
                    "{}: {}: {error}",
                    strings::ERR_LOAD_ROM_FAILED,
                    rom_path.display()
                );
                std::process::exit(1);
            }
        };

        let mut config = Config::load();
        // Update recent games list when started from CLI
        if let Some((title, is_cgb)) = save::parse_rom_header(&rom_path) {
            config.add_recent(rom_path.clone(), title, is_cgb);
            let _ = config.save();
        }

        let event_loop = EventLoop::new()?;
        let mut app = App::new(rom_path, rom, config);
        event_loop.run_app(&mut app)?;
    } else {
        launcher::run()?;
    }

    Ok(())
}
