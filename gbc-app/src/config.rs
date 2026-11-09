//! Emulator configuration, key bindings, and persistent state.
//!
//! Stored in TOML format using `dirs` and `std::path::Path`.
//! Fallbacks to defaults on missing or corrupted configuration.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use crate::strings;

/// Returns the configuration file path: `<config_dir>/gbcemu/config.toml`.
pub fn config_path() -> PathBuf {
    if let Some(mut path) = dirs::config_dir() {
        path.push("gbcemu");
        path.push("config.toml");
        path
    } else {
        PathBuf::from("gbcemu.toml")
    }
}

/// Returns the base configuration directory for the emulator.
pub fn config_dir() -> PathBuf {
    if let Some(mut path) = dirs::config_dir() {
        path.push("gbcemu");
        path
    } else {
        PathBuf::from(".")
    }
}

// ── Recent Games ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentGame {
    pub path: PathBuf,
    pub title: String,
    pub is_cgb: bool,
    pub last_played: String,
}

// ── Video Configuration ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub scale: u32,
    pub integer_scaling: bool,
    pub show_shell: bool,
    pub vsync: bool,
    #[serde(default)]
    pub shell_size: Option<(u32, u32)>,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            scale: 3,
            integer_scaling: false,
            show_shell: false,
            vsync: true,
            shell_size: None,
        }
    }
}

// ── DMG Palette ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DmgPalettePreset {
    #[default]
    ClassicGreen,
    Grayscale,
    Pocket,
    Custom,
}

impl DmgPalettePreset {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClassicGreen => strings::PALETTE_CLASSIC_GREEN,
            Self::Grayscale => strings::PALETTE_GRAYSCALE,
            Self::Pocket => strings::PALETTE_POCKET,
            Self::Custom => strings::PALETTE_CUSTOM,
        }
    }

    pub fn colors(&self, custom: &[[u8; 4]; 4]) -> [[u8; 4]; 4] {
        match self {
            Self::ClassicGreen => [
                [0xE0, 0xF8, 0xD0, 0xFF],
                [0x88, 0xC0, 0x70, 0xFF],
                [0x34, 0x68, 0x56, 0xFF],
                [0x08, 0x18, 0x20, 0xFF],
            ],
            Self::Grayscale => [
                [0xFF, 0xFF, 0xFF, 0xFF],
                [0xAA, 0xAA, 0xAA, 0xFF],
                [0x55, 0x55, 0x55, 0xFF],
                [0x00, 0x00, 0x00, 0xFF],
            ],
            Self::Pocket => [
                [0xC4, 0xCE, 0xA1, 0xFF],
                [0x8B, 0x95, 0x6D, 0xFF],
                [0x4D, 0x53, 0x3C, 0xFF],
                [0x1F, 0x1F, 0x1F, 0xFF],
            ],
            Self::Custom => *custom,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmgPaletteConfig {
    pub preset: DmgPalettePreset,
    pub custom_colors: [[u8; 4]; 4],
}

impl Default for DmgPaletteConfig {
    fn default() -> Self {
        Self {
            preset: DmgPalettePreset::ClassicGreen,
            custom_colors: [
                [0xE0, 0xF8, 0xD0, 0xFF],
                [0x88, 0xC0, 0x70, 0xFF],
                [0x34, 0x68, 0x56, 0xFF],
                [0x08, 0x18, 0x20, 0xFF],
            ],
        }
    }
}

// ── Audio Configuration ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub master_volume: f32,
    pub mute: bool,
    pub channel_1: bool,
    pub channel_2: bool,
    pub channel_3: bool,
    pub channel_4: bool,
    pub buffer_size: usize,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            mute: false,
            channel_1: true,
            channel_2: true,
            channel_3: true,
            channel_4: true,
            buffer_size: 1024,
        }
    }
}

// ── Emulation Configuration ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FastForwardSpeed {
    X2,
    #[default]
    X4,
    X8,
}

impl FastForwardSpeed {
    pub fn factor(&self) -> u32 {
        match self {
            Self::X2 => 2,
            Self::X4 => 4,
            Self::X8 => 8,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::X2 => strings::SPEED_2X,
            Self::X4 => strings::SPEED_4X,
            Self::X8 => strings::SPEED_8X,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulationConfig {
    pub fast_forward_speed: FastForwardSpeed,
}

impl Default for EmulationConfig {
    fn default() -> Self {
        Self {
            fast_forward_speed: FastForwardSpeed::X4,
        }
    }
}

// ── Paths Configuration ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SaveDirectoryMode {
    #[default]
    RomDirectory,
    ConfigDirectory,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathsConfig {
    pub save_directory_mode: SaveDirectoryMode,
}

// ── Controls Configuration ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameBoyControls {
    pub up: KeyCode,
    pub down: KeyCode,
    pub left: KeyCode,
    pub right: KeyCode,
    pub a: KeyCode,
    pub b: KeyCode,
    pub start: KeyCode,
    pub select: KeyCode,
}

impl Default for GameBoyControls {
    fn default() -> Self {
        Self {
            up: KeyCode::ArrowUp,
            down: KeyCode::ArrowDown,
            left: KeyCode::ArrowLeft,
            right: KeyCode::ArrowRight,
            a: KeyCode::KeyZ,
            b: KeyCode::KeyX,
            start: KeyCode::Enter,
            select: KeyCode::Backspace,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotkeyControls {
    pub fast_forward: KeyCode,
    pub pause: KeyCode,
    pub reset: KeyCode,
    pub save_state: KeyCode,
    pub load_state: KeyCode,
    pub screenshot: KeyCode,
    pub fullscreen: KeyCode,
}

impl Default for HotkeyControls {
    fn default() -> Self {
        Self {
            fast_forward: KeyCode::Tab,
            pause: KeyCode::KeyP,
            reset: KeyCode::KeyR,
            save_state: KeyCode::F5,
            load_state: KeyCode::F8,
            screenshot: KeyCode::F12,
            fullscreen: KeyCode::F11,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ControlsConfig {
    pub gameboy: GameBoyControls,
    pub hotkeys: HotkeyControls,
}

impl ControlsConfig {
    /// Returns all key codes currently in conflict (bound to more than one action).
    pub fn conflicts(&self) -> Vec<KeyCode> {
        let mut counts = HashMap::new();
        let all_keys = [
            self.gameboy.up,
            self.gameboy.down,
            self.gameboy.left,
            self.gameboy.right,
            self.gameboy.a,
            self.gameboy.b,
            self.gameboy.start,
            self.gameboy.select,
            self.hotkeys.fast_forward,
            self.hotkeys.pause,
            self.hotkeys.reset,
            self.hotkeys.save_state,
            self.hotkeys.load_state,
            self.hotkeys.screenshot,
            self.hotkeys.fullscreen,
        ];

        for &key in &all_keys {
            *counts.entry(key).or_insert(0) += 1;
        }

        counts
            .into_iter()
            .filter_map(|(key, count)| if count > 1 { Some(key) } else { None })
            .collect()
    }
}

// ── Full Application Config ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub recent_games: Vec<RecentGame>,
    #[serde(default)]
    pub library_folder: Option<PathBuf>,
    #[serde(default)]
    pub video: VideoConfig,
    #[serde(default)]
    pub dmg_palette: DmgPaletteConfig,
    #[serde(default)]
    pub cgb_color_correction: bool,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub emulation: EmulationConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub controls: ControlsConfig,
}

impl Config {
    /// Loads configuration from disk using `dirs`.
    /// Missing or corrupted configuration falls back to defaults without crashing.
    pub fn load() -> Self {
        let path = config_path();
        Self::load_from_path(&path)
    }

    /// Loads configuration from a specific path.
    pub fn load_from_path(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        match fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!(
                        "Warning: {}: {error}. Using defaults.",
                        strings::ERR_CONFIG_LOAD_FAILED
                    );
                    Self::default()
                }
            },
            Err(error) => {
                eprintln!(
                    "Warning: {}: {error}. Using defaults.",
                    strings::ERR_CONFIG_LOAD_FAILED
                );
                Self::default()
            }
        }
    }

    /// Persists configuration to disk.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = config_path();
        self.save_to_path(&path)
    }

    /// Persists configuration to a specific path.
    pub fn save_to_path(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let serialized = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(path, serialized)
    }

    /// Adds a game to the recent games list (max 10 entries).
    pub fn add_recent(&mut self, path: PathBuf, title: String, is_cgb: bool) {
        // Remove existing entry for the same path
        self.recent_games.retain(|g| g.path != path);

        let now_str = current_formatted_time();

        self.recent_games.insert(
            0,
            RecentGame {
                path,
                title,
                is_cgb,
                last_played: now_str,
            },
        );

        if self.recent_games.len() > 10 {
            self.recent_games.truncate(10);
        }
    }
}

/// Formats current local/system time as `YYYY-MM-DD HH:MM`.
pub fn current_formatted_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Civil time calculation from seconds since Unix epoch
    let seconds_in_day = 86400;
    let days_since_epoch = (now / seconds_in_day) as i64;
    let seconds_of_day = (now % seconds_in_day) as u32;

    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;

    // Convert days since 1970-01-01 to year, month, day
    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Gregorian calendar algorithm
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Returns a human-friendly Polish display name for a KeyCode.
pub fn key_display_name(key: KeyCode) -> &'static str {
    match key {
        KeyCode::ArrowUp => "↑ (Góra)",
        KeyCode::ArrowDown => "↓ (Dół)",
        KeyCode::ArrowLeft => "← (Lewo)",
        KeyCode::ArrowRight => "→ (Prawo)",
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyD => "D",
        KeyCode::KeyE => "E",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyI => "I",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyM => "M",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyP => "P",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyR => "R",
        KeyCode::KeyS => "S",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyW => "W",
        KeyCode::KeyX => "X",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Enter => "Enter",
        KeyCode::Space => "Spacja",
        KeyCode::Tab => "Tab",
        KeyCode::Backspace => "Backspace",
        KeyCode::Escape => "Escape",
        KeyCode::ShiftLeft => "Lewy Shift",
        KeyCode::ShiftRight => "Prawy Shift",
        KeyCode::ControlLeft => "Lewy Ctrl",
        KeyCode::ControlRight => "Prawy Ctrl",
        KeyCode::AltLeft => "Lewy Alt",
        KeyCode::AltRight => "Prawy Alt",
        KeyCode::F1 => "F1",
        KeyCode::F2 => "F2",
        KeyCode::F3 => "F3",
        KeyCode::F4 => "F4",
        KeyCode::F5 => "F5",
        KeyCode::F6 => "F6",
        KeyCode::F7 => "F7",
        KeyCode::F8 => "F8",
        KeyCode::F9 => "F9",
        KeyCode::F10 => "F10",
        KeyCode::F11 => "F11",
        KeyCode::F12 => "F12",
        KeyCode::Minus => "-",
        KeyCode::Equal => "=",
        _ => "(Klawisz)",
    }
}

/// Converts an `egui::Key` to a `winit::keyboard::KeyCode`.
pub fn egui_key_to_keycode(key: egui::Key) -> Option<KeyCode> {
    Some(match key {
        egui::Key::ArrowUp => KeyCode::ArrowUp,
        egui::Key::ArrowDown => KeyCode::ArrowDown,
        egui::Key::ArrowLeft => KeyCode::ArrowLeft,
        egui::Key::ArrowRight => KeyCode::ArrowRight,
        egui::Key::A => KeyCode::KeyA,
        egui::Key::B => KeyCode::KeyB,
        egui::Key::C => KeyCode::KeyC,
        egui::Key::D => KeyCode::KeyD,
        egui::Key::E => KeyCode::KeyE,
        egui::Key::F => KeyCode::KeyF,
        egui::Key::G => KeyCode::KeyG,
        egui::Key::H => KeyCode::KeyH,
        egui::Key::I => KeyCode::KeyI,
        egui::Key::J => KeyCode::KeyJ,
        egui::Key::K => KeyCode::KeyK,
        egui::Key::L => KeyCode::KeyL,
        egui::Key::M => KeyCode::KeyM,
        egui::Key::N => KeyCode::KeyN,
        egui::Key::O => KeyCode::KeyO,
        egui::Key::P => KeyCode::KeyP,
        egui::Key::Q => KeyCode::KeyQ,
        egui::Key::R => KeyCode::KeyR,
        egui::Key::S => KeyCode::KeyS,
        egui::Key::T => KeyCode::KeyT,
        egui::Key::U => KeyCode::KeyU,
        egui::Key::V => KeyCode::KeyV,
        egui::Key::W => KeyCode::KeyW,
        egui::Key::X => KeyCode::KeyX,
        egui::Key::Y => KeyCode::KeyY,
        egui::Key::Z => KeyCode::KeyZ,
        egui::Key::Num0 => KeyCode::Digit0,
        egui::Key::Num1 => KeyCode::Digit1,
        egui::Key::Num2 => KeyCode::Digit2,
        egui::Key::Num3 => KeyCode::Digit3,
        egui::Key::Num4 => KeyCode::Digit4,
        egui::Key::Num5 => KeyCode::Digit5,
        egui::Key::Num6 => KeyCode::Digit6,
        egui::Key::Num7 => KeyCode::Digit7,
        egui::Key::Num8 => KeyCode::Digit8,
        egui::Key::Num9 => KeyCode::Digit9,
        egui::Key::Enter => KeyCode::Enter,
        egui::Key::Space => KeyCode::Space,
        egui::Key::Tab => KeyCode::Tab,
        egui::Key::Backspace => KeyCode::Backspace,
        egui::Key::Escape => KeyCode::Escape,
        egui::Key::F1 => KeyCode::F1,
        egui::Key::F2 => KeyCode::F2,
        egui::Key::F3 => KeyCode::F3,
        egui::Key::F4 => KeyCode::F4,
        egui::Key::F5 => KeyCode::F5,
        egui::Key::F6 => KeyCode::F6,
        egui::Key::F7 => KeyCode::F7,
        egui::Key::F8 => KeyCode::F8,
        egui::Key::F9 => KeyCode::F9,
        egui::Key::F10 => KeyCode::F10,
        egui::Key::F11 => KeyCode::F11,
        egui::Key::F12 => KeyCode::F12,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let temp_dir = std::env::temp_dir().join("gbcemu_test_config");
        let path = temp_dir.join("config.toml");
        let _ = fs::remove_file(&path);

        let mut config = Config::default();
        config.video.scale = 5;
        config.video.integer_scaling = true;
        config.add_recent(PathBuf::from("/test/game.gb"), "TEST GAME".into(), true);

        assert!(config.save_to_path(&path).is_ok());
        let loaded = Config::load_from_path(&path);
        assert_eq!(loaded.video.scale, 5);
        assert!(loaded.video.integer_scaling);
        assert_eq!(loaded.recent_games.len(), 1);
        assert_eq!(loaded.recent_games[0].title, "TEST GAME");
        assert!(loaded.recent_games[0].is_cgb);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_corrupted_config_falls_back_to_defaults() {
        let temp_dir = std::env::temp_dir().join("gbcemu_test_corrupt");
        let path = temp_dir.join("corrupt.toml");
        let _ = fs::create_dir_all(&temp_dir);
        let _ = fs::write(&path, "this is definitely not valid toml !!! = = { [");

        let loaded = Config::load_from_path(&path);
        assert_eq!(loaded.video.scale, 3);
        assert_eq!(loaded.controls.gameboy.a, KeyCode::KeyZ);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_recent_games_limit_ten() {
        let mut config = Config::default();
        for i in 0..15 {
            config.add_recent(
                PathBuf::from(format!("/games/game_{i}.gb")),
                format!("GAME {i}"),
                false,
            );
        }
        assert_eq!(config.recent_games.len(), 10);
        assert_eq!(config.recent_games[0].title, "GAME 14");
    }

    #[test]
    fn test_conflict_detection() {
        let mut controls = ControlsConfig::default();
        assert!(controls.conflicts().is_empty());

        // Assign same key to two actions
        controls.gameboy.b = KeyCode::KeyZ; // Same as 'a'
        let conflicts = controls.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0], KeyCode::KeyZ);
    }
}
