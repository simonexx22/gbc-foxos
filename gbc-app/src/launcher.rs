//! Plain egui/eframe launcher window.
//!
//! Provides game selection, recently played list, library folder scanner,
//! video/audio/palette settings, and keyboard rebinding with conflict detection.
//! The console shell is strictly excluded from this window.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use winit::keyboard::KeyCode;

use crate::config::{
    self, key_display_name, Config, DmgPalettePreset, FastForwardSpeed, SaveDirectoryMode,
};
use crate::save;
use crate::strings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Games,
    Settings,
    Controls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebindingTarget {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Start,
    Select,
    FastForward,
    Pause,
    Reset,
    SaveState,
    LoadState,
    Screenshot,
    Fullscreen,
}

#[derive(Debug, Clone)]
struct LibraryEntry {
    path: PathBuf,
    title: String,
    is_cgb: bool,
    filename: String,
}

pub struct LauncherApp {
    config: Config,
    active_tab: Tab,
    selected_game: Option<(PathBuf, String, bool)>,
    library_games: Vec<LibraryEntry>,
    search_query: String,
    rebinding: Option<RebindingTarget>,
}

impl LauncherApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        let mut app = Self {
            config,
            active_tab: Tab::Games,
            selected_game: None,
            library_games: Vec::new(),
            search_query: String::new(),
            rebinding: None,
        };

        if let Some(folder) = app.config.library_folder.clone() {
            app.scan_library(&folder);
        }

        app
    }

    fn scan_library(&mut self, folder: &Path) {
        self.library_games.clear();
        let Ok(read_dir) = fs::read_dir(folder) else {
            return;
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext != "gb" && ext != "gbc" {
                continue;
            }

            let filename = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let (title, is_cgb) = save::parse_rom_header(&path).unwrap_or_else(|| {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| filename.clone());
                (stem, ext == "gbc")
            });

            self.library_games.push(LibraryEntry {
                path,
                title,
                is_cgb,
                filename,
            });
        }

        self.library_games.sort_by(|a, b| a.title.cmp(&b.title));
    }

    fn launch_game(&mut self, path: &Path, title: &str, is_cgb: bool, ctx: &egui::Context) {
        if !path.exists() {
            return;
        }

        self.config
            .add_recent(path.to_path_buf(), title.to_string(), is_cgb);
        let _ = self.config.save();

        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = Command::new(exe);
            cmd.arg(path);
            let _ = cmd.spawn();
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_drag_and_drop(&mut self, ctx: &egui::Context) {
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        for dropped in dropped_files {
            if let Some(path) = dropped.path {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if ext == "gb" || ext == "gbc" {
                    let (title, is_cgb) = save::parse_rom_header(&path).unwrap_or_else(|| {
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "ROM".into());
                        (stem, ext == "gbc")
                    });
                    self.selected_game = Some((path, title, is_cgb));
                }
            }
        }
    }

    fn handle_key_rebinding(&mut self, ctx: &egui::Context) {
        let Some(target) = self.rebinding else {
            return;
        };

        let events = ctx.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                ..
            } = event
            {
                if key == egui::Key::Escape {
                    self.rebinding = None;
                    return;
                }

                if let Some(keycode) = config::egui_key_to_keycode(key) {
                    match target {
                        RebindingTarget::Up => self.config.controls.gameboy.up = keycode,
                        RebindingTarget::Down => self.config.controls.gameboy.down = keycode,
                        RebindingTarget::Left => self.config.controls.gameboy.left = keycode,
                        RebindingTarget::Right => self.config.controls.gameboy.right = keycode,
                        RebindingTarget::A => self.config.controls.gameboy.a = keycode,
                        RebindingTarget::B => self.config.controls.gameboy.b = keycode,
                        RebindingTarget::Start => self.config.controls.gameboy.start = keycode,
                        RebindingTarget::Select => self.config.controls.gameboy.select = keycode,
                        RebindingTarget::FastForward => {
                            self.config.controls.hotkeys.fast_forward = keycode
                        }
                        RebindingTarget::Pause => self.config.controls.hotkeys.pause = keycode,
                        RebindingTarget::Reset => self.config.controls.hotkeys.reset = keycode,
                        RebindingTarget::SaveState => {
                            self.config.controls.hotkeys.save_state = keycode
                        }
                        RebindingTarget::LoadState => {
                            self.config.controls.hotkeys.load_state = keycode
                        }
                        RebindingTarget::Screenshot => {
                            self.config.controls.hotkeys.screenshot = keycode
                        }
                        RebindingTarget::Fullscreen => {
                            self.config.controls.hotkeys.fullscreen = keycode
                        }
                    }
                    self.rebinding = None;
                    let _ = self.config.save();
                    return;
                }
            }
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_drag_and_drop(ctx);
        self.handle_key_rebinding(ctx);

        egui::TopBottomPanel::top("top_navigation").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(strings::LAUNCHER_WINDOW_TITLE);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((path, title, is_cgb)) = &self.selected_game {
                        let exists = path.exists();
                        let btn = ui.add_enabled(
                            exists,
                            egui::Button::new(RichText::new(strings::BTN_PLAY).strong().size(15.0))
                                .min_size(Vec2::new(100.0, 30.0)),
                        );
                        if btn.clicked() {
                            let path_clone = path.clone();
                            let title_clone = title.clone();
                            let is_cgb_clone = *is_cgb;
                            self.launch_game(&path_clone, &title_clone, is_cgb_clone, ctx);
                        }
                    }
                });
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Games, strings::TAB_GAMES);
                ui.selectable_value(&mut self.active_tab, Tab::Settings, strings::TAB_SETTINGS);
                ui.selectable_value(&mut self.active_tab, Tab::Controls, strings::TAB_CONTROLS);
            });
            ui.add_space(6.0);
        });

        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some((path, title, _)) = &self.selected_game {
                    ui.label(format!(
                        "{}: {} ({})",
                        strings::COL_TITLE,
                        title,
                        path.display()
                    ));
                } else {
                    ui.weak(strings::DRAG_DROP_HINT);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Games => self.render_tab_games(ui, ctx),
            Tab::Settings => self.render_tab_settings(ui),
            Tab::Controls => self.render_tab_controls(ui),
        });
    }
}

impl LauncherApp {
    fn render_tab_games(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // "Otwórz ROM..." button
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new(strings::BTN_OPEN_ROM).strong())
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(strings::DIALOG_OPEN_ROM_TITLE)
                    .add_filter(strings::FILTER_ROMS_NAME, &["gb", "gbc"])
                    .pick_file()
                {
                    let (title, is_cgb) = save::parse_rom_header(&path).unwrap_or_else(|| {
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "ROM".into());
                        let is_cgb = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("gbc"))
                            .unwrap_or(false);
                        (stem, is_cgb)
                    });
                    self.selected_game = Some((path, title, is_cgb));
                }
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // "Ostatnio grane" section (last 10)
        ui.heading(strings::HEADER_RECENT_GAMES);
        let recent_items = self.config.recent_games.clone();

        if recent_items.is_empty() {
            ui.weak(strings::NO_RECENT_GAMES);
        } else {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .id_salt("recent_games_scroll")
                .show(ui, |ui| {
                    for game in &recent_items {
                        let exists = game.path.exists();
                        let is_selected = self
                            .selected_game
                            .as_ref()
                            .map(|(p, _, _)| p == &game.path)
                            .unwrap_or(false);

                        ui.horizontal(|ui| {
                            render_badge(ui, game.is_cgb);

                            let text = if exists {
                                RichText::new(&game.title).strong()
                            } else {
                                RichText::new(format!("{} {}", game.title, strings::FILE_NOT_FOUND))
                                    .strikethrough()
                                    .weak()
                            };

                            let resp = ui.selectable_label(is_selected, text);
                            if exists {
                                resp.clone()
                                    .on_hover_text(strings::TOOLTIP_DOUBLE_CLICK_TO_PLAY);
                                if resp.clicked() {
                                    self.selected_game =
                                        Some((game.path.clone(), game.title.clone(), game.is_cgb));
                                }
                                if resp.double_clicked() {
                                    self.launch_game(&game.path, &game.title, game.is_cgb, ctx);
                                }
                            } else {
                                resp.on_hover_text(strings::TOOLTIP_FILE_MISSING);
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.weak(&game.last_played);
                                },
                            );
                        });
                    }
                });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // Optional library folder section
        ui.heading(strings::HEADER_LIBRARY);
        ui.horizontal(|ui| {
            ui.label(strings::LABEL_LIBRARY_FOLDER);
            if let Some(folder) = &self.config.library_folder {
                ui.label(folder.display().to_string());
            } else {
                ui.weak(strings::NO_FOLDER_SELECTED);
            }

            if ui.button(strings::BTN_CHOOSE_FOLDER).clicked() {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_title(strings::DIALOG_CHOOSE_LIBRARY_TITLE)
                    .pick_folder()
                {
                    self.config.library_folder = Some(folder.clone());
                    let _ = self.config.save();
                    self.scan_library(&folder);
                }
            }
        });

        if self.config.library_folder.is_some() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(strings::SEARCH_PLACEHOLDER);
                ui.text_edit_singleline(&mut self.search_query);
            });

            ui.add_space(4.0);
            let filter = self.search_query.trim().to_lowercase();
            let games_to_display: Vec<LibraryEntry> = self
                .library_games
                .iter()
                .filter(|g| {
                    if filter.is_empty() {
                        true
                    } else {
                        g.title.to_lowercase().contains(&filter)
                            || g.filename.to_lowercase().contains(&filter)
                    }
                })
                .cloned()
                .collect();

            if games_to_display.is_empty() {
                ui.weak(strings::NO_LIBRARY_GAMES);
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("library_games_scroll")
                    .show(ui, |ui| {
                        for game in games_to_display {
                            let is_selected = self
                                .selected_game
                                .as_ref()
                                .map(|(p, _, _)| p == &game.path)
                                .unwrap_or(false);

                            ui.horizontal(|ui| {
                                render_badge(ui, game.is_cgb);

                                let resp = ui.selectable_label(
                                    is_selected,
                                    RichText::new(&game.title).strong(),
                                );
                                resp.clone()
                                    .on_hover_text(strings::TOOLTIP_DOUBLE_CLICK_TO_PLAY);

                                if resp.clicked() {
                                    self.selected_game =
                                        Some((game.path.clone(), game.title.clone(), game.is_cgb));
                                }
                                if resp.double_clicked() {
                                    self.launch_game(&game.path, &game.title, game.is_cgb, ctx);
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.weak(&game.filename);
                                    },
                                );
                            });
                        }
                    });
            }
        }
    }

    fn render_tab_settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .show(ui, |ui| {
                // 1. Video
                ui.heading(strings::GROUP_VIDEO);
                ui.horizontal(|ui| {
                    ui.label(strings::LABEL_WINDOW_SCALE);
                    let mut scale = self.config.video.scale;
                    if ui.add(egui::Slider::new(&mut scale, 1..=6)).changed() {
                        self.config.video.scale = scale;
                        let _ = self.config.save();
                    }
                });

                if ui
                    .checkbox(
                        &mut self.config.video.integer_scaling,
                        strings::LABEL_INTEGER_SCALING,
                    )
                    .on_hover_text(strings::TOOLTIP_INTEGER_SCALING)
                    .changed()
                {
                    let _ = self.config.save();
                }

                if ui
                    .checkbox(&mut self.config.video.show_shell, strings::LABEL_SHOW_SHELL)
                    .on_hover_text(strings::TOOLTIP_SHOW_SHELL)
                    .changed()
                {
                    let _ = self.config.save();
                }

                if ui
                    .checkbox(&mut self.config.video.vsync, strings::LABEL_VSYNC)
                    .on_hover_text(strings::TOOLTIP_VSYNC)
                    .changed()
                {
                    let _ = self.config.save();
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // 2. DMG Palette
                ui.heading(strings::GROUP_DMG_PALETTE);
                ui.horizontal(|ui| {
                    ui.label(strings::LABEL_DMG_PRESET);
                    let mut preset = self.config.dmg_palette.preset;
                    egui::ComboBox::from_id_salt("dmg_palette_combo")
                        .selected_text(preset.name())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut preset,
                                DmgPalettePreset::ClassicGreen,
                                strings::PALETTE_CLASSIC_GREEN,
                            );
                            ui.selectable_value(
                                &mut preset,
                                DmgPalettePreset::Grayscale,
                                strings::PALETTE_GRAYSCALE,
                            );
                            ui.selectable_value(
                                &mut preset,
                                DmgPalettePreset::Pocket,
                                strings::PALETTE_POCKET,
                            );
                            ui.selectable_value(
                                &mut preset,
                                DmgPalettePreset::Custom,
                                strings::PALETTE_CUSTOM,
                            );
                        });

                    if preset != self.config.dmg_palette.preset {
                        self.config.dmg_palette.preset = preset;
                        let _ = self.config.save();
                    }
                });

                // Live Color Preview
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(strings::LABEL_PALETTE_PREVIEW);
                    let colors = self
                        .config
                        .dmg_palette
                        .preset
                        .colors(&self.config.dmg_palette.custom_colors);
                    for color in &colors {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(32.0, 20.0), Sense::hover());
                        ui.painter().rect_filled(
                            rect,
                            3.0,
                            Color32::from_rgba_premultiplied(
                                color[0], color[1], color[2], color[3],
                            ),
                        );
                        ui.painter().rect_stroke(
                            rect,
                            3.0,
                            Stroke::new(1.0_f32, Color32::from_gray(120)),
                            egui::StrokeKind::Outside,
                        );
                    }
                });

                // Custom Color Pickers if Custom is active
                if self.config.dmg_palette.preset == DmgPalettePreset::Custom {
                    ui.add_space(4.0);
                    ui.label(strings::LABEL_CUSTOM_COLORS);
                    let shades = [
                        strings::SHADE_WHITE,
                        strings::SHADE_LIGHT_GRAY,
                        strings::SHADE_DARK_GRAY,
                        strings::SHADE_BLACK,
                    ];
                    for (i, &shade) in shades.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(shade);
                            let mut rgb = [
                                self.config.dmg_palette.custom_colors[i][0],
                                self.config.dmg_palette.custom_colors[i][1],
                                self.config.dmg_palette.custom_colors[i][2],
                            ];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                self.config.dmg_palette.custom_colors[i] =
                                    [rgb[0], rgb[1], rgb[2], 0xFF];
                                let _ = self.config.save();
                            }
                        });
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // 3. CGB
                ui.heading(strings::GROUP_CGB);
                if ui
                    .checkbox(
                        &mut self.config.cgb_color_correction,
                        strings::LABEL_CGB_COLOR_CORRECTION,
                    )
                    .on_hover_text(strings::TOOLTIP_CGB_COLOR_CORRECTION)
                    .changed()
                {
                    let _ = self.config.save();
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // 4. Audio
                ui.heading(strings::GROUP_AUDIO);
                ui.horizontal(|ui| {
                    ui.label(strings::LABEL_MASTER_VOLUME);
                    let mut vol = (self.config.audio.master_volume * 100.0).round() as u32;
                    if ui
                        .add(egui::Slider::new(&mut vol, 0..=100).suffix("%"))
                        .changed()
                    {
                        self.config.audio.master_volume = vol as f32 / 100.0;
                        let _ = self.config.save();
                    }
                });

                if ui
                    .checkbox(&mut self.config.audio.mute, strings::LABEL_MUTE)
                    .changed()
                {
                    let _ = self.config.save();
                }

                ui.add_space(4.0);
                ui.label(strings::LABEL_AUDIO_CHANNELS);
                if ui
                    .checkbox(&mut self.config.audio.channel_1, strings::LABEL_CHANNEL_1)
                    .changed()
                {
                    let _ = self.config.save();
                }
                if ui
                    .checkbox(&mut self.config.audio.channel_2, strings::LABEL_CHANNEL_2)
                    .changed()
                {
                    let _ = self.config.save();
                }
                if ui
                    .checkbox(&mut self.config.audio.channel_3, strings::LABEL_CHANNEL_3)
                    .changed()
                {
                    let _ = self.config.save();
                }
                if ui
                    .checkbox(&mut self.config.audio.channel_4, strings::LABEL_CHANNEL_4)
                    .changed()
                {
                    let _ = self.config.save();
                }

                ui.horizontal(|ui| {
                    ui.label(strings::LABEL_BUFFER_SIZE);
                    let mut size = self.config.audio.buffer_size;
                    egui::ComboBox::from_id_salt("audio_buffer_size_combo")
                        .selected_text(format!("{size} {}", strings::SAMPLES_UNIT))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut size,
                                512,
                                format!("512 {}", strings::SAMPLES_UNIT),
                            );
                            ui.selectable_value(
                                &mut size,
                                1024,
                                format!("1024 {}", strings::SAMPLES_UNIT),
                            );
                            ui.selectable_value(
                                &mut size,
                                2048,
                                format!("2048 {}", strings::SAMPLES_UNIT),
                            );
                            ui.selectable_value(
                                &mut size,
                                4096,
                                format!("4096 {}", strings::SAMPLES_UNIT),
                            );
                        });
                    if size != self.config.audio.buffer_size {
                        self.config.audio.buffer_size = size;
                        let _ = self.config.save();
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // 5. Emulation
                ui.heading(strings::GROUP_EMULATION);
                ui.horizontal(|ui| {
                    ui.label(strings::LABEL_FAST_FORWARD_SPEED);
                    let mut speed = self.config.emulation.fast_forward_speed;
                    ui.radio_value(&mut speed, FastForwardSpeed::X2, strings::SPEED_2X);
                    ui.radio_value(&mut speed, FastForwardSpeed::X4, strings::SPEED_4X);
                    ui.radio_value(&mut speed, FastForwardSpeed::X8, strings::SPEED_8X);
                    if speed != self.config.emulation.fast_forward_speed {
                        self.config.emulation.fast_forward_speed = speed;
                        let _ = self.config.save();
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // 6. Paths
                ui.heading(strings::GROUP_PATHS);
                ui.label(strings::LABEL_SAVE_DIRECTORY);
                let mut mode = self.config.paths.save_directory_mode;
                ui.radio_value(
                    &mut mode,
                    SaveDirectoryMode::RomDirectory,
                    strings::SAVE_DIR_ROM,
                );
                ui.radio_value(
                    &mut mode,
                    SaveDirectoryMode::ConfigDirectory,
                    strings::SAVE_DIR_CONFIG,
                );
                if mode != self.config.paths.save_directory_mode {
                    self.config.paths.save_directory_mode = mode;
                    let _ = self.config.save();
                }
                ui.label(format!(
                    "{}: {}",
                    strings::LABEL_CURRENT_CONFIG_DIR,
                    config::config_dir().display()
                ));
            });
    }

    fn render_tab_controls(&mut self, ui: &mut egui::Ui) {
        let conflicts = self.config.controls.conflicts();

        ui.horizontal(|ui| {
            if ui.button(strings::BTN_RESTORE_DEFAULTS).clicked() {
                self.config.controls = Default::default();
                let _ = self.config.save();
            }

            if !conflicts.is_empty() {
                ui.colored_label(Color32::RED, strings::CONFLICT_WARNING);
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("controls_scroll")
            .show(ui, |ui| {
                ui.heading(strings::GROUP_GAMEBOY_BUTTONS);
                self.render_rebind_row(
                    ui,
                    strings::ACTION_UP,
                    self.config.controls.gameboy.up,
                    RebindingTarget::Up,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_DOWN,
                    self.config.controls.gameboy.down,
                    RebindingTarget::Down,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_LEFT,
                    self.config.controls.gameboy.left,
                    RebindingTarget::Left,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_RIGHT,
                    self.config.controls.gameboy.right,
                    RebindingTarget::Right,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_A,
                    self.config.controls.gameboy.a,
                    RebindingTarget::A,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_B,
                    self.config.controls.gameboy.b,
                    RebindingTarget::B,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_START,
                    self.config.controls.gameboy.start,
                    RebindingTarget::Start,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::ACTION_SELECT,
                    self.config.controls.gameboy.select,
                    RebindingTarget::Select,
                    &conflicts,
                );

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                ui.heading(strings::GROUP_HOTKEYS);
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_FAST_FORWARD,
                    self.config.controls.hotkeys.fast_forward,
                    RebindingTarget::FastForward,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_PAUSE,
                    self.config.controls.hotkeys.pause,
                    RebindingTarget::Pause,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_RESET,
                    self.config.controls.hotkeys.reset,
                    RebindingTarget::Reset,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_SAVE_STATE,
                    self.config.controls.hotkeys.save_state,
                    RebindingTarget::SaveState,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_LOAD_STATE,
                    self.config.controls.hotkeys.load_state,
                    RebindingTarget::LoadState,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_SCREENSHOT,
                    self.config.controls.hotkeys.screenshot,
                    RebindingTarget::Screenshot,
                    &conflicts,
                );
                self.render_rebind_row(
                    ui,
                    strings::HOTKEY_FULLSCREEN,
                    self.config.controls.hotkeys.fullscreen,
                    RebindingTarget::Fullscreen,
                    &conflicts,
                );
            });
    }

    fn render_rebind_row(
        &mut self,
        ui: &mut egui::Ui,
        action_name: &str,
        key: KeyCode,
        target: RebindingTarget,
        conflicts: &[KeyCode],
    ) {
        let is_rebinding = self.rebinding == Some(target);
        let has_conflict = conflicts.contains(&key);

        ui.horizontal(|ui| {
            ui.label(RichText::new(action_name).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let btn_text = if is_rebinding {
                    RichText::new(strings::PRESS_A_KEY).color(Color32::YELLOW)
                } else if has_conflict {
                    RichText::new(format!("{} (!!!)", key_display_name(key))).color(Color32::RED)
                } else {
                    RichText::new(key_display_name(key))
                };

                let btn = ui.button(btn_text).on_hover_text(strings::TOOLTIP_REBIND);
                if btn.clicked() {
                    if is_rebinding {
                        self.rebinding = None;
                    } else {
                        self.rebinding = Some(target);
                    }
                }
            });
        });
    }
}

fn render_badge(ui: &mut egui::Ui, is_cgb: bool) {
    let (badge_text, bg_color, text_color) = if is_cgb {
        (
            strings::BADGE_CGB,
            Color32::from_rgb(46, 16, 101),
            Color32::from_rgb(192, 132, 252),
        )
    } else {
        (
            strings::BADGE_DMG,
            Color32::from_rgb(20, 83, 45),
            Color32::from_rgb(134, 239, 172),
        )
    };

    let font_id = egui::FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(badge_text.to_string(), font_id, text_color);

    let padding = Vec2::new(6.0, 2.0);
    let desired_size = galley.size() + padding * 2.0;
    let (rect, _) = ui.allocate_exact_size(desired_size, Sense::hover());

    ui.painter().rect_filled(rect, 4.0, bg_color);
    ui.painter().galley(rect.min + padding, galley, text_color);
}

/// Runs the launcher window natively using `eframe`.
pub fn run() -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(strings::LAUNCHER_WINDOW_TITLE)
            .with_inner_size([720.0, 520.0])
            .with_min_inner_size([540.0, 380.0]),
        ..Default::default()
    };

    eframe::run_native(
        strings::LAUNCHER_WINDOW_TITLE,
        native_options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_text_from_shapes(shapes: &[egui::epaint::ClippedShape]) -> String {
        let mut full_text = String::new();
        for clipped in shapes {
            extract_shape_text(&clipped.shape, &mut full_text);
        }
        full_text
    }

    fn extract_shape_text(shape: &egui::epaint::Shape, out: &mut String) {
        match shape {
            egui::epaint::Shape::Text(text_shape) => {
                out.push_str(text_shape.galley.text());
                out.push(' ');
            }
            egui::epaint::Shape::Vec(shapes) => {
                for s in shapes {
                    extract_shape_text(s, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn test_launcher_tabs_render_and_display_polish_characters() {
        let ctx = egui::Context::default();
        let mut app = LauncherApp {
            config: Config::default(),
            active_tab: Tab::Games,
            selected_game: Some((
                PathBuf::from("/games/gra.gb"),
                "SUPER GRA".to_string(),
                false,
            )),
            library_games: vec![LibraryEntry {
                path: PathBuf::from("/games/gra.gb"),
                title: "SUPER GRA".to_string(),
                is_cgb: false,
                filename: "gra.gb".to_string(),
            }],
            search_query: String::new(),
            rebinding: None,
        };

        // Render Tab::Games
        app.active_tab = Tab::Games;
        let output_games = ctx.run(egui::RawInput::default(), |ctx| {
            app.render_tab_games(
                &mut egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("test_games"),
                    egui::UiBuilder::default(),
                ),
                ctx,
            );
        });
        let text_games = extract_text_from_shapes(&output_games.shapes);
        // Verify key Polish words in Games tab
        assert!(
            text_games.contains(strings::BTN_OPEN_ROM),
            "Must contain 'Otwórz ROM...'"
        );
        assert!(
            text_games.contains(strings::HEADER_RECENT_GAMES),
            "Must contain 'Ostatnio grane'"
        );

        // Render Tab::Settings
        app.active_tab = Tab::Settings;
        let output_settings = ctx.run(egui::RawInput::default(), |ctx| {
            app.render_tab_settings(&mut egui::Ui::new(
                ctx.clone(),
                egui::Id::new("test_settings"),
                egui::UiBuilder::default(),
            ));
        });
        let text_settings = extract_text_from_shapes(&output_settings.shapes);
        // Verify Polish characters in Settings tab:
        // ą: 'Głośność'
        // ć: 'Dźwięk', 'prędkość'
        // ę: 'Dźwięk', 'zieleń'
        // ł: 'Główna'
        // ń: 'odcień', 'zieleń'
        // ó: 'Główny', 'skrótów'
        // ś: 'Głośność'
        // ź: 'Dźwięk'
        // ż: 'bufora'
        assert!(
            text_settings.contains(strings::GROUP_VIDEO),
            "Must contain 'Obraz'"
        );
        assert!(
            text_settings.contains(strings::GROUP_AUDIO),
            "Must contain 'Dźwięk'"
        );
        assert!(
            text_settings.contains(strings::LABEL_FAST_FORWARD_SPEED),
            "Must contain 'Prędkość szybkiego przewijania'"
        );

        // Render Tab::Controls
        app.active_tab = Tab::Controls;
        let output_controls = ctx.run(egui::RawInput::default(), |ctx| {
            app.render_tab_controls(&mut egui::Ui::new(
                ctx.clone(),
                egui::Id::new("test_controls"),
                egui::UiBuilder::default(),
            ));
        });
        let text_controls = extract_text_from_shapes(&output_controls.shapes);
        assert!(
            text_controls.contains(strings::BTN_RESTORE_DEFAULTS),
            "Must contain 'Przywróć domyślne'"
        );
        assert!(
            text_controls.contains(strings::HOTKEY_FAST_FORWARD),
            "Must contain 'Szybkie przewijanie'"
        );
        assert!(
            text_controls.contains(strings::ACTION_UP),
            "Must contain 'W górę'"
        );
        assert!(
            text_controls.contains(strings::ACTION_DOWN),
            "Must contain 'W dół'"
        );

        // Verify that all 9 Polish diacritics (ą, ć, ę, ł, ń, ó, ś, ź, ż) are present across the UI tabs!
        let all_ui_text = format!("{text_games} {text_settings} {text_controls}");
        for ch in ['ą', 'ć', 'ę', 'ł', 'ń', 'ó', 'ś', 'ź', 'ż'] {
            assert!(
                all_ui_text.contains(ch),
                "Polish character '{ch}' must be present in launcher UI text"
            );
        }
    }
}
