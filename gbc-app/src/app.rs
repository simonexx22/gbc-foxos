//! winit event loop, window, frameless shell presentation, and user controls.
//!
//! Frame pacing: `about_to_wait` advances the emulator by one frame and
//! paces emulation using dynamic rate control tied to the audio buffer.

#![allow(clippy::chunks_exact_to_as_chunks)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gbc_core::{Button, Gameboy, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE, SCREEN_HEIGHT, SCREEN_WIDTH};
use pixels::wgpu;
use pixels::{Pixels, PixelsBuilder, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::audio::AudioOutput;
use crate::config::Config;
use crate::font;
use crate::input;
use crate::save;
use crate::strings;

/// Game Boy frame duration: 59.7275 Hz ≈ 4_194_304 / 70_224 cycles.
const FRAME_INTERVAL: Duration = Duration::from_nanos(16_742_706);

/// Shell reference dimensions in the SVG coordinate space.
const SHELL_ASPECT_W: f64 = 500.0;
const SHELL_ASPECT_H: f64 = 800.0;
const SHELL_ASPECT_RATIO: f64 = 500.0 / 800.0;

/// Screen hole geometry in the 500x800 shell (exact 10:9 aspect ratio, 70% width).
const HOLE_X: f64 = 75.0;
const HOLE_Y: f64 = 65.0;
const HOLE_W: f64 = 350.0;
const HOLE_H: f64 = 315.0;

/// Master shell PNG bytes (500x800 RGBA).
const SHELL_PNG_BYTES: &[u8] = include_bytes!("../assets/shell.png");

/// Hit-test results for mouse clicks on the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellHit {
    Screen,
    Button(Button),
    Reset,
    Body,
}

/// Tests mouse coordinates (in window space) against shell controls.
fn hit_test(mx: f64, my: f64, win_w: f64, win_h: f64) -> ShellHit {
    let nx = mx * (SHELL_ASPECT_W / win_w.max(1.0));
    let ny = my * (SHELL_ASPECT_H / win_h.max(1.0));

    // Screen hole (game screen area)
    if (HOLE_X..=HOLE_X + HOLE_W).contains(&nx) && (HOLE_Y..=HOLE_Y + HOLE_H).contains(&ny) {
        return ShellHit::Screen;
    }

    // D-Pad (Center at X=125, Y=555)
    // Up arm
    if (105.0..=145.0).contains(&nx) && (500.0..=545.0).contains(&ny) {
        return ShellHit::Button(Button::Up);
    }
    // Down arm
    if (105.0..=145.0).contains(&nx) && (565.0..=610.0).contains(&ny) {
        return ShellHit::Button(Button::Down);
    }
    // Left arm
    if (70.0..=115.0).contains(&nx) && (535.0..=575.0).contains(&ny) {
        return ShellHit::Button(Button::Left);
    }
    // Right arm
    if (135.0..=180.0).contains(&nx) && (535.0..=575.0).contains(&ny) {
        return ShellHit::Button(Button::Right);
    }

    // B Button (Center at X=335, Y=580, radius=35)
    let dx_b = nx - 335.0;
    let dy_b = ny - 580.0;
    if dx_b * dx_b + dy_b * dy_b <= 35.0 * 35.0 {
        return ShellHit::Button(Button::B);
    }

    // A Button (Center at X=415, Y=515, radius=35)
    let dx_a = nx - 415.0;
    let dy_a = ny - 515.0;
    if dx_a * dx_a + dy_a * dy_a <= 35.0 * 35.0 {
        return ShellHit::Button(Button::A);
    }

    // Reset Button (Center at X=190, Y=695)
    if (160.0..=220.0).contains(&nx) && (680.0..=715.0).contains(&ny) {
        return ShellHit::Reset;
    }

    // Start Button (Center at X=275, Y=695)
    if (245.0..=305.0).contains(&nx) && (680.0..=715.0).contains(&ny) {
        return ShellHit::Button(Button::Start);
    }

    // Default: Click on shell body -> triggers window dragging
    ShellHit::Body
}

/// Precomputes 1D look-up table for sharp bilinear scaling.
fn compute_sharp_bilinear_lut(src_len: usize, dst_len: usize) -> Vec<(usize, usize, f32)> {
    if dst_len == 0 {
        return Vec::new();
    }
    // Sharp integer multiplier: at least 4x, or up to ratio
    let m = (dst_len as f64 / src_len as f64).ceil().max(4.0) * 2.0;
    let scale = src_len as f64 / dst_len as f64;
    let mut lut = Vec::with_capacity(dst_len);

    for dx in 0..dst_len {
        let u = (dx as f64 + 0.5) * scale - 0.5;
        let u_floor = u.floor();
        let u0 = if u_floor >= 0.0 {
            (u_floor as usize).min(src_len - 1)
        } else {
            0
        };
        let u1 = (u0 + 1).min(src_len - 1);
        let frac = if u_floor >= 0.0 { u - u_floor } else { 0.0 };
        let w = ((frac - 0.5) * m + 0.5).clamp(0.0, 1.0) as f32;
        lut.push((u0, u1, w));
    }

    lut
}

/// Blits source 160x144 frame into destination buffer using sharp bilinear interpolation.
#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
fn blit_sharp_bilinear(
    src: &[u8],
    dst: &mut [u8],
    dst_stride: usize,
    dst_x: usize,
    dst_y: usize,
    dst_w: usize,
    dst_h: usize,
    x_lut: &[(usize, usize, f32)],
    y_lut: &[(usize, usize, f32)],
) {
    for dy in 0..dst_h {
        let (y0, y1, wy) = y_lut[dy];
        let py = dst_y + dy;
        let dst_row_offset = (py * dst_stride + dst_x) * 4;

        let src_row0 = y0 * SCREEN_WIDTH * 4;
        let src_row1 = y1 * SCREEN_WIDTH * 4;

        for dx in 0..dst_w {
            let (x0, x1, wx) = x_lut[dx];
            let out_idx = dst_row_offset + dx * 4;

            let idx00 = src_row0 + x0 * 4;
            let idx10 = src_row0 + x1 * 4;
            let idx01 = src_row1 + x0 * 4;
            let idx11 = src_row1 + x1 * 4;

            let inv_wx = 1.0 - wx;
            let inv_wy = 1.0 - wy;

            let w00 = inv_wx * inv_wy;
            let w10 = wx * inv_wy;
            let w01 = inv_wx * wy;
            let w11 = wx * wy;

            let r = (f32::from(src[idx00]) * w00
                + f32::from(src[idx10]) * w10
                + f32::from(src[idx01]) * w01
                + f32::from(src[idx11]) * w11)
                .round() as u8;

            let g = (f32::from(src[idx00 + 1]) * w00
                + f32::from(src[idx10 + 1]) * w10
                + f32::from(src[idx01 + 1]) * w01
                + f32::from(src[idx11 + 1]) * w11)
                .round() as u8;

            let b = (f32::from(src[idx00 + 2]) * w00
                + f32::from(src[idx10 + 2]) * w10
                + f32::from(src[idx01 + 2]) * w01
                + f32::from(src[idx11 + 2]) * w11)
                .round() as u8;

            dst[out_idx..out_idx + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

/// Blits source 160x144 frame into destination buffer with strict integer scaling.
fn blit_integer_scale(
    src: &[u8],
    dst: &mut [u8],
    dst_stride: usize,
    dst_x: usize,
    dst_y: usize,
    scale: usize,
) {
    for sy in 0..SCREEN_HEIGHT {
        let src_row = sy * SCREEN_WIDTH * 4;
        for sx in 0..SCREEN_WIDTH {
            let src_idx = src_row + sx * 4;
            let pixel = &src[src_idx..src_idx + 4];

            for dy in 0..scale {
                let py = dst_y + sy * scale + dy;
                let row_start = (py * dst_stride + dst_x + sx * scale) * 4;
                for dx in 0..scale {
                    let out_idx = row_start + dx * 4;
                    dst[out_idx..out_idx + 4].copy_from_slice(pixel);
                }
            }
        }
    }
}

/// Resamples the 500x800 master shell PNG to arbitrary destination buffer dimensions.
fn scale_shell_image(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
) {
    if dst_w == 0 || dst_h == 0 {
        return;
    }

    let x_scale = src_w as f64 / dst_w as f64;
    let y_scale = src_h as f64 / dst_h as f64;

    for dy in 0..dst_h {
        let sy = ((dy as f64 + 0.5) * y_scale).floor() as usize;
        let sy = sy.min(src_h - 1);
        let src_row = sy * src_w * 4;
        let dst_row = dy * dst_w * 4;

        for dx in 0..dst_w {
            let sx = ((dx as f64 + 0.5) * x_scale).floor() as usize;
            let sx = sx.min(src_w - 1);
            let src_idx = src_row + sx * 4;
            let dst_idx = dst_row + dx * 4;

            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }
}

/// Frontend application state.
pub struct App {
    rom_path: PathBuf,
    save_path: PathBuf,
    gameboy: Gameboy,
    config: Config,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    audio: Option<AudioOutput>,
    next_frame: Instant,
    paused: bool,
    fast_forwarding: bool,
    base_title: String,
    message: Option<(String, Instant)>,

    // Shell state
    shell_master: Option<Vec<u8>>,
    scaled_shell: Option<Vec<u8>>,
    current_size: (u32, u32),
    transparent_supported: bool,
    active_shell_button: Option<Button>,
    modifiers: ModifiersState,
    cursor_pos: (f64, f64),

    // Context menu state
    menu_open: bool,
    menu_pos: (f64, f64),
    menu_hovered_index: Option<usize>,

    // Sharp bilinear look-up tables
    lut_x: Vec<(usize, usize, f32)>,
    lut_y: Vec<(usize, usize, f32)>,
    game_rect: (u32, u32, u32, u32), // (x, y, w, h)
}

impl App {
    /// Creates the application, restores save RAM, and applies configuration.
    pub fn new(rom_path: PathBuf, rom: Vec<u8>, config: Config) -> Self {
        let save_path = save::save_path(&rom_path, &config.paths);
        let mut gameboy = Gameboy::new(rom);

        // Apply DMG palette
        let colors = config
            .dmg_palette
            .preset
            .colors(&config.dmg_palette.custom_colors);
        gameboy.set_dmg_palette(colors);

        // Apply CGB color correction
        gameboy.set_cgb_color_correction(config.cgb_color_correction);

        // Apply APU channel enables
        gameboy.set_channel_enabled(1, config.audio.channel_1);
        gameboy.set_channel_enabled(2, config.audio.channel_2);
        gameboy.set_channel_enabled(3, config.audio.channel_3);
        gameboy.set_channel_enabled(4, config.audio.channel_4);

        if let Ok(data) = save::load(&save_path) {
            if !data.is_empty() {
                gameboy.set_save_ram(&data);
            }
        }

        let base_title = match rom_path.file_stem() {
            Some(stem) => format!(
                "{} – {}",
                strings::EMULATOR_WINDOW_TITLE_PREFIX,
                stem.to_string_lossy()
            ),
            None => strings::EMULATOR_WINDOW_TITLE_PREFIX.to_string(),
        };

        let shell_master = if config.video.show_shell {
            image::load_from_memory_with_format(SHELL_PNG_BYTES, image::ImageFormat::Png)
                .ok()
                .map(|img| img.to_rgba8().into_raw())
        } else {
            None
        };

        Self {
            rom_path,
            save_path,
            gameboy,
            config,
            window: None,
            pixels: None,
            audio: None,
            next_frame: Instant::now(),
            paused: false,
            fast_forwarding: false,
            base_title,
            message: None,
            shell_master,
            scaled_shell: None,
            current_size: (0, 0),
            transparent_supported: false,
            active_shell_button: None,
            modifiers: ModifiersState::default(),
            cursor_pos: (0.0, 0.0),
            menu_open: false,
            menu_pos: (0.0, 0.0),
            menu_hovered_index: None,
            lut_x: Vec::new(),
            lut_y: Vec::new(),
            game_rect: (0, 0, 0, 0),
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let scale = self.config.video.scale.max(1);
        let show_shell = self.config.video.show_shell;

        // Calculate initial window size
        let (init_w, init_h) = if show_shell {
            if let Some((w, h)) = self.config.video.shell_size {
                (w.clamp(350, 1200), h.clamp(560, 1920))
            } else {
                // Default size where game appears at ~3x - 4x scale
                // At width 750, height 1200: hole is 525x472.5 -> 3.28x game
                let def_w = (500.0 * (scale as f64 * 0.5).max(1.2)).round() as u32;
                let def_h = ((def_w as f64) / SHELL_ASPECT_RATIO).round() as u32;
                (def_w.clamp(350, 1200), def_h.clamp(560, 1920))
            }
        } else {
            (SCREEN_WIDTH as u32 * scale, SCREEN_HEIGHT as u32 * scale)
        };

        let mut attributes = Window::default_attributes()
            .with_title(&self.base_title)
            .with_inner_size(LogicalSize::new(init_w as f64, init_h as f64));

        if show_shell {
            // Frameless and transparent for console shell
            attributes = attributes
                .with_decorations(false)
                .with_transparent(true)
                .with_min_inner_size(LogicalSize::new(350.0, 560.0))
                .with_max_inner_size(LogicalSize::new(1200.0, 1920.0));
        } else {
            attributes = attributes
                .with_decorations(true)
                .with_min_inner_size(LogicalSize::new(SCREEN_WIDTH as f64, SCREEN_HEIGHT as f64));
        }

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("{}: {error}", strings::ERR_CREATE_WINDOW);
                event_loop.exit();
                return;
            }
        };

        // Window title is set even when borderless so it shows in the taskbar
        window.set_title(&self.base_title);

        let size = window.inner_size();
        let surface_w = size.width.max(1);
        let surface_h = size.height.max(1);

        let surface_texture = SurfaceTexture::new(surface_w, surface_h, Arc::clone(&window));

        let (pixels, is_transparent) = if show_shell {
            // Try transparent surface with premultiplied alpha
            let builder = PixelsBuilder::new(surface_w, surface_h, surface_texture)
                .clear_color(wgpu::Color::TRANSPARENT)
                .blend_state(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
                .alpha_mode(wgpu::CompositeAlphaMode::PreMultiplied)
                .enable_vsync(self.config.video.vsync);

            match builder.build() {
                Ok(p) => (p, true),
                Err(error) => {
                    eprintln!(
                        "Transparency is not supported on this system ({error}); falling back to opaque dark window."
                    );
                    let fallback_surface =
                        SurfaceTexture::new(surface_w, surface_h, Arc::clone(&window));
                    let p = PixelsBuilder::new(surface_w, surface_h, fallback_surface)
                        .clear_color(wgpu::Color {
                            r: 0.08,
                            g: 0.08,
                            b: 0.08,
                            a: 1.0,
                        })
                        .blend_state(wgpu::BlendState::ALPHA_BLENDING)
                        .alpha_mode(wgpu::CompositeAlphaMode::Auto)
                        .enable_vsync(self.config.video.vsync)
                        .build()
                        .expect("failed to create fallback opaque pixels surface");
                    (p, false)
                }
            }
        } else {
            let p = PixelsBuilder::new(surface_w, surface_h, surface_texture)
                .clear_color(wgpu::Color::BLACK)
                .enable_vsync(self.config.video.vsync)
                .build()
                .expect("failed to create pixels surface");
            (p, false)
        };

        self.audio = match AudioOutput::new(self.config.audio.buffer_size) {
            Ok(audio) => {
                if audio.sample_rate() != AUDIO_SAMPLE_RATE || audio.channels() != AUDIO_CHANNELS {
                    eprintln!(
                        "audio device is {} Hz/{} ch; core produces {} Hz/{} ch",
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
        self.transparent_supported = is_transparent;
        self.on_buffer_resized(surface_w, surface_h);

        self.next_frame = Instant::now();
    }

    /// Updates internal scaling buffers and look-up tables when dimensions change.
    fn on_buffer_resized(&mut self, width: u32, height: u32) {
        self.current_size = (width, height);

        if self.config.video.show_shell {
            if let Some(master) = &self.shell_master {
                let mut scaled = vec![0u8; (width * height * 4) as usize];
                scale_shell_image(
                    master,
                    SHELL_ASPECT_W as usize,
                    SHELL_ASPECT_H as usize,
                    &mut scaled,
                    width as usize,
                    height as usize,
                );
                self.scaled_shell = Some(scaled);
            }

            // Hole in buffer coordinates
            let hole_x = (f64::from(width) * (HOLE_X / SHELL_ASPECT_W)).round() as u32;
            let hole_y = (f64::from(height) * (HOLE_Y / SHELL_ASPECT_H)).round() as u32;
            let hole_w = (f64::from(width) * (HOLE_W / SHELL_ASPECT_W)).round() as u32;
            let hole_h = (f64::from(height) * (HOLE_H / SHELL_ASPECT_H)).round() as u32;

            if self.config.video.integer_scaling {
                let k = (hole_w / SCREEN_WIDTH as u32)
                    .min(hole_h / SCREEN_HEIGHT as u32)
                    .max(1);
                let gw = SCREEN_WIDTH as u32 * k;
                let gh = SCREEN_HEIGHT as u32 * k;
                let gx = hole_x + (hole_w.saturating_sub(gw)) / 2;
                let gy = hole_y + (hole_h.saturating_sub(gh)) / 2;
                self.game_rect = (gx, gy, gw, gh);
            } else {
                self.game_rect = (hole_x, hole_y, hole_w, hole_h);
                self.lut_x = compute_sharp_bilinear_lut(SCREEN_WIDTH, hole_w as usize);
                self.lut_y = compute_sharp_bilinear_lut(SCREEN_HEIGHT, hole_h as usize);
            }
        } else {
            self.game_rect = (0, 0, width, height);
            self.lut_x = compute_sharp_bilinear_lut(SCREEN_WIDTH, width as usize);
            self.lut_y = compute_sharp_bilinear_lut(SCREEN_HEIGHT, height as usize);
        }
    }

    /// Sets an in-game HUD notification message.
    fn set_message(&mut self, msg: String) {
        self.message = Some((msg, Instant::now()));
    }

    /// Returns to the launcher window and closes the emulator window.
    fn return_to_launcher(&mut self, event_loop: &ActiveEventLoop) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe).spawn();
        }
        event_loop.exit();
    }

    /// Emulate one frame, blit to pixel buffer, and render.
    fn render_frame(&mut self) {
        if !self.paused {
            let cycles_count = if self.fast_forwarding {
                self.config.emulation.fast_forward_speed.factor()
            } else {
                1
            };

            for _ in 0..cycles_count {
                self.gameboy.step_frame();
            }

            if let Some(audio) = self.audio.as_mut() {
                audio.push_samples(
                    self.gameboy.audio_samples(),
                    self.config.audio.master_volume,
                    self.config.audio.mute || self.fast_forwarding,
                );
            }
        }

        if let Some(window) = &self.window {
            window.pre_present_notify();
        }

        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };

        let (win_w, win_h) = self.current_size;
        let stride = win_w as usize;
        let frame = pixels.frame_mut();

        if self.config.video.show_shell {
            // Clear entire buffer: transparent if supported, dark grey otherwise
            if self.transparent_supported {
                frame.fill(0);
            } else {
                for chunk in frame.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&[20, 20, 20, 255]);
                }
            }

            // Fill hole area with opaque black background
            let hole_x = (f64::from(win_w) * (HOLE_X / SHELL_ASPECT_W)).round() as usize;
            let hole_y = (f64::from(win_h) * (HOLE_Y / SHELL_ASPECT_H)).round() as usize;
            let hole_w = (f64::from(win_w) * (HOLE_W / SHELL_ASPECT_W)).round() as usize;
            let hole_h = (f64::from(win_h) * (HOLE_H / SHELL_ASPECT_H)).round() as usize;

            for y in hole_y..(hole_y + hole_h).min(win_h as usize) {
                let row_start = (y * stride + hole_x) * 4;
                let row_len = hole_w.min(stride - hole_x) * 4;
                for chunk in frame[row_start..row_start + row_len].chunks_exact_mut(4) {
                    chunk.copy_from_slice(&[0, 0, 0, 255]);
                }
            }

            // Render game into the hole
            let (gx, gy, gw, gh) = self.game_rect;
            let gb_frame = self.gameboy.framebuffer();

            if self.config.video.integer_scaling {
                let k = (gw / SCREEN_WIDTH as u32).max(1) as usize;
                blit_integer_scale(gb_frame, frame, stride, gx as usize, gy as usize, k);
            } else if !self.lut_x.is_empty() && !self.lut_y.is_empty() {
                blit_sharp_bilinear(
                    gb_frame,
                    frame,
                    stride,
                    gx as usize,
                    gy as usize,
                    gw as usize,
                    gh as usize,
                    &self.lut_x,
                    &self.lut_y,
                );
            }

            // Composite shell over the frame (opaque shell body covers borders, transparent hole reveals game)
            if let Some(shell) = &self.scaled_shell {
                for (dst_px, src_px) in frame.chunks_exact_mut(4).zip(shell.chunks_exact(4)) {
                    if src_px[3] > 0 {
                        dst_px.copy_from_slice(src_px);
                    }
                }
            }
        } else {
            // Non-shell direct rendering
            let gb_frame = self.gameboy.framebuffer();
            if self.config.video.integer_scaling {
                let k = (win_w / SCREEN_WIDTH as u32)
                    .min(win_h / SCREEN_HEIGHT as u32)
                    .max(1) as usize;
                let ox = (win_w as usize - SCREEN_WIDTH * k) / 2;
                let oy = (win_h as usize - SCREEN_HEIGHT * k) / 2;
                frame.fill(0);
                blit_integer_scale(gb_frame, frame, stride, ox, oy, k);
            } else if !self.lut_x.is_empty() && !self.lut_y.is_empty() {
                blit_sharp_bilinear(
                    gb_frame,
                    frame,
                    stride,
                    0,
                    0,
                    win_w as usize,
                    win_h as usize,
                    &self.lut_x,
                    &self.lut_y,
                );
            }
        }

        // Render right-click context menu if open
        if self.menu_open {
            Self::draw_context_menu(
                frame,
                stride,
                win_w,
                win_h,
                self.menu_pos,
                self.menu_hovered_index,
                self.paused,
            );
        }

        // Render HUD message banner if active
        if let Some((msg, time)) = &self.message {
            if time.elapsed() < Duration::from_secs(2) {
                let y = if self.config.video.show_shell {
                    (f64::from(win_h) * 0.44).round() as usize
                } else {
                    12
                };
                let x = 16;
                // Draw pill banner background
                let banner_w = msg.chars().count() * 8 + 16;
                for by in y..y + 20 {
                    if by < win_h as usize {
                        let row_start = (by * stride + x) * 4;
                        let row_len = banner_w.min(stride.saturating_sub(x)) * 4;
                        if row_start + row_len <= frame.len() {
                            for px in frame[row_start..row_start + row_len].chunks_exact_mut(4) {
                                px.copy_from_slice(&[15, 23, 42, 230]);
                            }
                        }
                    }
                }
                font::draw_text(frame, stride, x + 8, y + 3, msg, [248, 250, 252, 255], 1);
            }
        }

        if let Err(error) = pixels.render() {
            eprintln!("render error: {error}");
        }
    }

    /// Draws the right-click popup menu into the frame buffer.
    fn draw_context_menu(
        frame: &mut [u8],
        stride: usize,
        win_w: u32,
        win_h: u32,
        menu_pos: (f64, f64),
        menu_hovered_index: Option<usize>,
        paused: bool,
    ) {
        let menu_w = 190usize;
        let menu_h = 120usize;
        let mx = (menu_pos.0 as usize)
            .min(win_w.saturating_sub(menu_w as u32 + 5) as usize)
            .max(5);
        let my = (menu_pos.1 as usize)
            .min(win_h.saturating_sub(menu_h as u32 + 5) as usize)
            .max(5);

        // Draw menu background box
        for y in my..my + menu_h {
            if y < win_h as usize {
                let row_start = (y * stride + mx) * 4;
                let row_len = menu_w.min(stride.saturating_sub(mx)) * 4;
                if row_start + row_len <= frame.len() {
                    for (x_offset, px) in frame[row_start..row_start + row_len]
                        .chunks_exact_mut(4)
                        .enumerate()
                    {
                        // Border
                        if y == my
                            || y == my + menu_h - 1
                            || x_offset == 0
                            || x_offset == menu_w - 1
                        {
                            px.copy_from_slice(&[71, 85, 105, 255]); // slate border
                        } else {
                            px.copy_from_slice(&[15, 23, 42, 245]); // dark slate body
                        }
                    }
                }
            }
        }

        let items = [
            if paused {
                strings::MENU_RESUME
            } else {
                strings::MENU_PAUSE
            },
            strings::MENU_RESET,
            strings::MENU_BACK_TO_LAUNCHER,
            strings::MENU_CLOSE,
        ];

        let item_h = 26usize;
        for (idx, &item_text) in items.iter().enumerate() {
            let row_y = my + 8 + idx * item_h;
            let is_hovered = menu_hovered_index == Some(idx);

            // Highlight hovered row
            if is_hovered {
                for hy in row_y..row_y + item_h - 2 {
                    if hy < win_h as usize {
                        let row_start = (hy * stride + mx + 2) * 4;
                        let row_len = (menu_w - 4).min(stride.saturating_sub(mx + 2)) * 4;
                        if row_start + row_len <= frame.len() {
                            for px in frame[row_start..row_start + row_len].chunks_exact_mut(4) {
                                px.copy_from_slice(&[51, 65, 85, 255]);
                            }
                        }
                    }
                }
            }

            let text_color = if is_hovered {
                [255, 255, 255, 255]
            } else {
                [226, 232, 240, 255]
            };
            font::draw_text(frame, stride, mx + 12, row_y + 5, item_text, text_color, 1);
        }
    }

    /// Handles right-click context menu click.
    fn handle_menu_click(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(idx) = self.menu_hovered_index {
            match idx {
                0 => {
                    // Pause / Resume
                    self.paused = !self.paused;
                    if self.paused {
                        if let Some(audio) = &self.audio {
                            let _ = audio.pause();
                        }
                    } else {
                        if let Some(audio) = &self.audio {
                            let _ = audio.play();
                        }
                        self.next_frame = Instant::now();
                    }
                    self.set_message(
                        if self.paused {
                            strings::MSG_PAUSED
                        } else {
                            strings::MSG_RESUMED
                        }
                        .to_string(),
                    );
                }
                1 => {
                    // Reset
                    self.gameboy.reset();
                    let colors = self
                        .config
                        .dmg_palette
                        .preset
                        .colors(&self.config.dmg_palette.custom_colors);
                    self.gameboy.set_dmg_palette(colors);
                    self.gameboy
                        .set_cgb_color_correction(self.config.cgb_color_correction);
                    self.set_message(strings::MSG_RESET.to_string());
                }
                2 => {
                    // Back to launcher
                    self.return_to_launcher(event_loop);
                }
                3 => {
                    // Close
                    event_loop.exit();
                }
                _ => {}
            }
        }
        self.menu_open = false;
        self.menu_hovered_index = None;
    }

    fn persist_save(&self) {
        if let Err(error) = save::store(&self.save_path, self.gameboy.save_ram()) {
            eprintln!(
                "{}: {}: {error}",
                strings::ERR_WRITE_SAVE_FAILED,
                self.save_path.display()
            );
        }
    }

    fn take_screenshot(&mut self) {
        let (win_w, win_h) = self.current_size;
        let Some(pixels) = self.pixels.as_ref() else {
            return;
        };
        let frame = pixels.frame();

        let screenshots_dir = save::save_path(&self.rom_path, &self.config.paths)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("screenshots"));
        let _ = std::fs::create_dir_all(&screenshots_dir);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let stem = self
            .rom_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "game".into());
        let path = screenshots_dir.join(format!("{stem}_{timestamp}.png"));

        if let Some(img) = image::RgbaImage::from_raw(win_w, win_h, frame.to_vec()) {
            if img.save(&path).is_ok() {
                self.set_message(format!(
                    "{}: {}",
                    strings::MSG_SCREENSHOT_SAVED,
                    path.display()
                ));
            } else {
                self.set_message(strings::MSG_SCREENSHOT_FAILED.to_string());
            }
        }
    }

    /// Resizes the shell window by the given width delta, maintaining the exact 5:8 aspect ratio.
    fn resize_shell_by(&mut self, delta_w: f64) {
        let (cur_w, _) = self.current_size;
        let new_w = ((cur_w as f64 + delta_w).clamp(350.0, 1200.0)).round() as u32;
        let new_h = ((new_w as f64) / SHELL_ASPECT_RATIO).round() as u32;

        if let Some(window) = &self.window {
            let _ = window.request_inner_size(LogicalSize::new(new_w as f64, new_h as f64));
        }

        self.config.video.shell_size = Some((new_w, new_h));
        let _ = self.config.save();
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
        if self.window.is_none() {
            return;
        }

        let now = Instant::now();
        if now >= self.next_frame {
            self.render_frame();

            let interval = if self.fast_forwarding {
                FRAME_INTERVAL
            } else {
                let factor = if let Some(audio) = &self.audio {
                    audio.dynamic_rate_multiplier()
                } else {
                    1.0
                };
                FRAME_INTERVAL.mul_f64(factor)
            };

            self.next_frame += interval;
            if self.next_frame < now && (now - self.next_frame) > Duration::from_millis(100) {
                self.next_frame = now + interval;
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
                    let _ = pixels.resize_surface(size.width, size.height);
                    let _ = pixels.resize_buffer(size.width, size.height);
                }
                self.on_buffer_resized(size.width, size.height);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x, position.y);

                // Update context menu hover
                if self.menu_open {
                    let (win_w, win_h) = self.current_size;
                    let menu_w = 190.0;
                    let menu_h = 120.0;
                    let mx = self.menu_pos.0.min(win_w as f64 - menu_w - 5.0).max(5.0);
                    let my = self.menu_pos.1.min(win_h as f64 - menu_h - 5.0).max(5.0);

                    if position.x >= mx
                        && position.x <= mx + menu_w
                        && position.y >= my + 8.0
                        && position.y <= my + menu_h - 8.0
                    {
                        let idx = ((position.y - (my + 8.0)) / 26.0).floor() as usize;
                        if idx < 4 {
                            self.menu_hovered_index = Some(idx);
                        } else {
                            self.menu_hovered_index = None;
                        }
                    } else {
                        self.menu_hovered_index = None;
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Right && state == ElementState::Pressed {
                    // Open or reposition right-click context menu
                    self.menu_open = true;
                    self.menu_pos = self.cursor_pos;
                    self.menu_hovered_index = None;
                    return;
                }

                if button == MouseButton::Left {
                    if state == ElementState::Pressed {
                        if self.menu_open {
                            self.handle_menu_click(event_loop);
                            return;
                        }

                        if self.config.video.show_shell {
                            let (win_w, win_h) = self.current_size;
                            let hit = hit_test(
                                self.cursor_pos.0,
                                self.cursor_pos.1,
                                win_w as f64,
                                win_h as f64,
                            );

                            match hit {
                                ShellHit::Button(btn) => {
                                    self.gameboy.set_button(btn, true);
                                    self.active_shell_button = Some(btn);
                                }
                                ShellHit::Reset => {
                                    self.gameboy.reset();
                                    let colors = self
                                        .config
                                        .dmg_palette
                                        .preset
                                        .colors(&self.config.dmg_palette.custom_colors);
                                    self.gameboy.set_dmg_palette(colors);
                                    self.gameboy
                                        .set_cgb_color_correction(self.config.cgb_color_correction);
                                    self.set_message(strings::MSG_RESET.to_string());
                                }
                                ShellHit::Screen => {
                                    // Click inside screen hole: no dragging
                                }
                                ShellHit::Body => {
                                    // Click on shell body: drag window
                                    if let Some(window) = &self.window {
                                        let _ = window.drag_window();
                                    }
                                }
                            }
                        }
                    } else if state == ElementState::Released {
                        if let Some(btn) = self.active_shell_button.take() {
                            self.gameboy.set_button(btn, false);
                        }
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.config.video.show_shell {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => {
                            if p.y > 0.0 {
                                1.0
                            } else if p.y < 0.0 {
                                -1.0
                            } else {
                                0.0
                            }
                        }
                    };

                    if dy.abs() > 0.0 {
                        let step = if dy > 0.0 { 30.0 } else { -30.0 };
                        self.resize_shell_by(step);
                    }
                }
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let is_pressed = event.state == ElementState::Pressed;

                    if is_pressed {
                        // Alt+F4 or Ctrl+Q closes the emulator
                        if (code == KeyCode::F4 && self.modifiers.alt_key())
                            || (code == KeyCode::KeyQ && self.modifiers.control_key())
                        {
                            event_loop.exit();
                            return;
                        }

                        // Esc returns to the launcher
                        if code == KeyCode::Escape {
                            self.return_to_launcher(event_loop);
                            return;
                        }

                        // +/- keys resize the shell window
                        if self.config.video.show_shell {
                            if code == KeyCode::Equal || code == KeyCode::NumpadAdd {
                                self.resize_shell_by(30.0);
                                return;
                            }
                            if code == KeyCode::Minus || code == KeyCode::NumpadSubtract {
                                self.resize_shell_by(-30.0);
                                return;
                            }
                        }

                        // Hotkeys
                        if code == self.config.controls.hotkeys.pause {
                            self.paused = !self.paused;
                            if self.paused {
                                if let Some(audio) = &self.audio {
                                    let _ = audio.pause();
                                }
                            } else {
                                if let Some(audio) = &self.audio {
                                    let _ = audio.play();
                                }
                                self.next_frame = Instant::now();
                            }
                            self.set_message(
                                if self.paused {
                                    strings::MSG_PAUSED
                                } else {
                                    strings::MSG_RESUMED
                                }
                                .to_string(),
                            );
                            return;
                        }

                        if code == self.config.controls.hotkeys.reset {
                            self.gameboy.reset();
                            let colors = self
                                .config
                                .dmg_palette
                                .preset
                                .colors(&self.config.dmg_palette.custom_colors);
                            self.gameboy.set_dmg_palette(colors);
                            self.gameboy
                                .set_cgb_color_correction(self.config.cgb_color_correction);
                            self.gameboy
                                .set_channel_enabled(1, self.config.audio.channel_1);
                            self.gameboy
                                .set_channel_enabled(2, self.config.audio.channel_2);
                            self.gameboy
                                .set_channel_enabled(3, self.config.audio.channel_3);
                            self.gameboy
                                .set_channel_enabled(4, self.config.audio.channel_4);
                            self.set_message(strings::MSG_RESET.to_string());
                            return;
                        }

                        if code == self.config.controls.hotkeys.save_state {
                            let state_path = save::state_path(&self.rom_path, &self.config.paths);
                            let state = self.gameboy.save_state();
                            if let Some(parent) = state_path.parent() {
                                if !parent.as_os_str().is_empty() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                            }
                            if std::fs::write(&state_path, state).is_ok() {
                                self.set_message(format!(
                                    "{}: {}",
                                    strings::MSG_STATE_SAVED,
                                    state_path.display()
                                ));
                            } else {
                                self.set_message(strings::MSG_STATE_SAVE_FAILED.to_string());
                            }
                            return;
                        }

                        if code == self.config.controls.hotkeys.load_state {
                            let state_path = save::state_path(&self.rom_path, &self.config.paths);
                            match std::fs::read(&state_path) {
                                Ok(data) => match self.gameboy.load_state(&data) {
                                    Ok(()) => {
                                        self.set_message(strings::MSG_STATE_LOADED.to_string())
                                    }
                                    Err(_) => {
                                        self.set_message(strings::MSG_STATE_LOAD_FAILED.to_string())
                                    }
                                },
                                Err(_) => {
                                    self.set_message(strings::MSG_STATE_LOAD_FAILED.to_string());
                                }
                            }
                            return;
                        }

                        if code == self.config.controls.hotkeys.screenshot {
                            self.take_screenshot();
                            return;
                        }

                        if code == self.config.controls.hotkeys.fullscreen {
                            if let Some(window) = &self.window {
                                let is_fullscreen = window.fullscreen().is_some();
                                window.set_fullscreen(if is_fullscreen {
                                    None
                                } else {
                                    Some(Fullscreen::Borderless(None))
                                });
                            }
                            return;
                        }
                    }

                    if code == self.config.controls.hotkeys.fast_forward {
                        self.fast_forwarding = is_pressed;
                        return;
                    }

                    if let Some(button) =
                        input::button_for_key(event.physical_key, &self.config.controls.gameboy)
                    {
                        self.gameboy.set_button(button, is_pressed);
                    }
                }
            }

            WindowEvent::RedrawRequested => {}

            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_aspect_ratio_and_hole_proportions() {
        assert_eq!(SHELL_ASPECT_RATIO, 500.0 / 800.0);
        assert_eq!(HOLE_W / SHELL_ASPECT_W, 0.70); // 70% of body width
        assert_eq!(
            (HOLE_W / HOLE_H * 1000.0).round(),
            (10.0f64 / 9.0 * 1000.0).round()
        ); // exact 10:9
    }

    #[test]
    fn test_shell_hit_testing() {
        let w = 500.0;
        let h = 800.0;

        // Inside screen hole
        assert_eq!(hit_test(250.0, 200.0, w, h), ShellHit::Screen);

        // D-Pad Up
        assert_eq!(hit_test(125.0, 520.0, w, h), ShellHit::Button(Button::Up));
        // D-Pad Down
        assert_eq!(hit_test(125.0, 580.0, w, h), ShellHit::Button(Button::Down));
        // D-Pad Left
        assert_eq!(hit_test(90.0, 555.0, w, h), ShellHit::Button(Button::Left));
        // D-Pad Right
        assert_eq!(
            hit_test(160.0, 555.0, w, h),
            ShellHit::Button(Button::Right)
        );

        // A Button
        assert_eq!(hit_test(415.0, 515.0, w, h), ShellHit::Button(Button::A));
        // B Button
        assert_eq!(hit_test(335.0, 580.0, w, h), ShellHit::Button(Button::B));

        // Reset Button
        assert_eq!(hit_test(190.0, 695.0, w, h), ShellHit::Reset);
        // Start Button
        assert_eq!(
            hit_test(275.0, 695.0, w, h),
            ShellHit::Button(Button::Start)
        );

        // Shell body (outside buttons and screen)
        assert_eq!(hit_test(50.0, 50.0, w, h), ShellHit::Body);
        assert_eq!(hit_test(250.0, 20.0, w, h), ShellHit::Body);
        assert_eq!(hit_test(250.0, 450.0, w, h), ShellHit::Body);
    }

    #[test]
    fn test_sharp_bilinear_lut() {
        let lut = compute_sharp_bilinear_lut(160, 350);
        assert_eq!(lut.len(), 350);
        for &(u0, u1, w) in &lut {
            assert!(u0 < 160);
            assert!(u1 < 160);
            assert!((0.0..=1.0).contains(&w));
        }
    }
}
