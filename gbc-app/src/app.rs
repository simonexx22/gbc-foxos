//! winit event loop, window, and frame presentation.
//!
//! Frame pacing: `about_to_wait` advances the emulator by one frame and
//! asks the event loop to wake up again at `next_frame`, yielding a
//! steady ~59.7 Hz cadence regardless of display vsync.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gbc_core::{Gameboy, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE, SCREEN_HEIGHT, SCREEN_WIDTH};
use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::audio::AudioOutput;
use crate::input;
use crate::save;

/// Game Boy frame duration: 59.7275 Hz ≈ 4_194_304 / 70_224 cycles.
const FRAME_INTERVAL: Duration = Duration::from_nanos(16_742_706);

/// Frontend application state.
pub struct App {
    rom_path: PathBuf,
    save_path: PathBuf,
    gameboy: Gameboy,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    audio: Option<AudioOutput>,
    /// Instant at which the next emulated frame should be displayed.
    next_frame: Instant,
}

impl App {
    /// Creates the application and restores any existing save RAM.
    pub fn new(rom_path: PathBuf, rom: Vec<u8>) -> Self {
        let save_path = save::save_path(&rom_path);
        let mut gameboy = Gameboy::new(rom);

        match save::load(&save_path) {
            Ok(data) if !data.is_empty() => gameboy.set_save_ram(&data),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                eprintln!("failed to load save file {}: {error}", save_path.display());
            }
        }

        Self {
            rom_path,
            save_path,
            gameboy,
            window: None,
            pixels: None,
            audio: None,
            next_frame: Instant::now(),
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let title = match self.rom_path.file_stem() {
            Some(stem) => format!("gbcemu - {}", stem.to_string_lossy()),
            None => "gbcemu".to_string(),
        };

        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(
                SCREEN_WIDTH as f64 * 3.0,
                SCREEN_HEIGHT as f64 * 3.0,
            ))
            .with_min_inner_size(LogicalSize::new(SCREEN_WIDTH as f64, SCREEN_HEIGHT as f64));

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };

        let size = window.inner_size();
        let surface =
            SurfaceTexture::new(size.width.max(1), size.height.max(1), Arc::clone(&window));

        let pixels = match Pixels::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32, surface) {
            Ok(pixels) => pixels,
            Err(error) => {
                eprintln!("failed to create pixel surface: {error}");
                event_loop.exit();
                return;
            }
        };

        self.audio = match AudioOutput::new() {
            Ok(audio) => {
                if audio.sample_rate() != AUDIO_SAMPLE_RATE || audio.channels() != AUDIO_CHANNELS {
                    eprintln!(
                        "audio device is {} Hz/{} ch; core produces {} Hz/{} ch (resampling TODO)",
                        audio.sample_rate(),
                        audio.channels(),
                        AUDIO_SAMPLE_RATE,
                        AUDIO_CHANNELS
                    );
                }
                Some(audio)
            }
            Err(error) => {
                eprintln!("audio disabled: {error}");
                None
            }
        };

        self.window = Some(window);
        self.pixels = Some(pixels);

        // Kick off the frame-pacing loop.
        self.next_frame = Instant::now();
    }

    /// Emulate one frame and blit it to the window.
    fn render_frame(&mut self) {
        self.gameboy.step_frame();

        if let Some(audio) = &self.audio {
            audio.push_samples(self.gameboy.audio_samples());
        }

        if let Some(window) = &self.window {
            window.pre_present_notify();
        }

        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        pixels
            .frame_mut()
            .copy_from_slice(self.gameboy.framebuffer());

        if let Err(error) = pixels.render() {
            eprintln!("render error: {error}");
        }
    }

    fn persist_save(&self) {
        if let Err(error) = save::store(&self.save_path, self.gameboy.save_ram()) {
            eprintln!(
                "failed to write save file {}: {error}",
                self.save_path.display()
            );
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.persist_save();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.initialize(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Run the emulator frame and present it, then schedule the next wakeup
        // at exactly next_frame so we maintain the ~59.7 Hz cadence.
        if self.window.is_none() {
            return;
        }

        let now = Instant::now();
        if now >= self.next_frame {
            self.render_frame();

            // Advance deadline; skip frames if we're running behind.
            self.next_frame += FRAME_INTERVAL;
            if self.next_frame < now {
                // We fell behind by more than one frame; resync.
                self.next_frame = now + FRAME_INTERVAL;
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }

                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(error) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("failed to resize surface: {error}");
                        event_loop.exit();
                    }
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => {
                            event_loop.exit();
                            return;
                        }
                        PhysicalKey::Code(KeyCode::F5) => {
                            let state_path = save::state_path(&self.rom_path);
                            let state = self.gameboy.save_state();
                            if let Err(e) = std::fs::write(&state_path, state) {
                                eprintln!("failed to save state to {}: {e}", state_path.display());
                            } else {
                                println!("saved state to {}", state_path.display());
                            }
                            return;
                        }
                        PhysicalKey::Code(KeyCode::F8) => {
                            let state_path = save::state_path(&self.rom_path);
                            match std::fs::read(&state_path) {
                                Ok(data) => match self.gameboy.load_state(&data) {
                                    Ok(()) => {
                                        println!("loaded state from {}", state_path.display())
                                    }
                                    Err(e) => eprintln!("failed to load state: {e}"),
                                },
                                Err(e) => eprintln!(
                                    "failed to read state file {}: {e}",
                                    state_path.display()
                                ),
                            }
                            return;
                        }
                        _ => {}
                    }
                }

                if let Some(button) = input::button_for_key(event.physical_key) {
                    self.gameboy
                        .set_button(button, event.state == ElementState::Pressed);
                }
            }

            // We drive rendering from about_to_wait, not from RedrawRequested,
            // so there is nothing to do here.
            WindowEvent::RedrawRequested => {}

            _ => {}
        }
    }
}
