//! Polish localization strings for the Game Boy Color emulator and launcher.
//!
//! All user-facing strings must be kept here so another language can be added later.

// ── Window Titles ─────────────────────────────────────────────────────────────
pub const LAUNCHER_WINDOW_TITLE: &str = "gbcemu – Menu główne";
pub const EMULATOR_WINDOW_TITLE_PREFIX: &str = "gbcemu";

// ── Launcher Tabs ─────────────────────────────────────────────────────────────
pub const TAB_GAMES: &str = "Gry";
pub const TAB_SETTINGS: &str = "Ustawienia";
pub const TAB_CONTROLS: &str = "Sterowanie";

// ── Tab 1: Gry (Games) ─────────────────────────────────────────────────────────
pub const BTN_OPEN_ROM: &str = "Otwórz ROM...";
pub const BTN_PLAY: &str = "Graj";
pub const HEADER_RECENT_GAMES: &str = "Ostatnio grane";
pub const HEADER_LIBRARY: &str = "Biblioteka gier";
pub const LABEL_LIBRARY_FOLDER: &str = "Katalog biblioteki:";
pub const BTN_CHOOSE_FOLDER: &str = "Wybierz folder...";
pub const SEARCH_PLACEHOLDER: &str = "Szukaj...";
pub const NO_RECENT_GAMES: &str = "Brak ostatnio granych gier";
pub const NO_LIBRARY_GAMES: &str = "Brak gier w wybranym katalogu";
pub const NO_FOLDER_SELECTED: &str = "Nie wybrano katalogu biblioteki";
pub const FILE_NOT_FOUND: &str = "(brak pliku)";
pub const DRAG_DROP_HINT: &str = "Przeciągnij i upuść plik .gb lub .gbc tutaj";
pub const BADGE_DMG: &str = "DMG";
pub const BADGE_CGB: &str = "CGB";
pub const COL_TITLE: &str = "Tytuł";
pub const COL_FORMAT: &str = "Format";
pub const COL_LAST_PLAYED: &str = "Ostatnio uruchomiono";
pub const COL_PATH: &str = "Ścieżka";
pub const TOOLTIP_DOUBLE_CLICK_TO_PLAY: &str = "Kliknij dwukrotnie, aby uruchomić grę";
pub const TOOLTIP_FILE_MISSING: &str = "Plik nie istnieje pod podaną ścieżką";

// ── Tab 2: Ustawienia (Settings) ───────────────────────────────────────────────
// Video
pub const GROUP_VIDEO: &str = "Obraz";
pub const LABEL_WINDOW_SCALE: &str = "Skala okna";
pub const LABEL_INTEGER_SCALING: &str = "Skalowanie całkowitoliczbowe";
pub const LABEL_SHOW_SHELL: &str = "Pokazuj obudowę konsoli";
pub const LABEL_VSYNC: &str = "Synchronizacja pionowa (V-Sync)";
pub const TOOLTIP_WINDOW_SCALE: &str = "Wybierz współczynnik powiększenia okna gry (1x - 6x)";
pub const TOOLTIP_INTEGER_SCALING: &str =
    "Skaluj obraz z zachowaniem pełnych wielokrotności pikseli";
pub const TOOLTIP_SHOW_SHELL: &str = "Wyświetlaj ramkę stylizowaną na retro konsolę przenośną";
pub const TOOLTIP_VSYNC: &str = "Włącz lub wyłącz synchronizację klatek z częstotliwością monitora";

// DMG Palette
pub const GROUP_DMG_PALETTE: &str = "Paleta DMG (Game Boy)";
pub const LABEL_DMG_PRESET: &str = "Profil kolorów:";
pub const PALETTE_CLASSIC_GREEN: &str = "Klasyczna zieleń";
pub const PALETTE_GRAYSCALE: &str = "Skala szarości";
pub const PALETTE_POCKET: &str = "Pocket";
pub const PALETTE_CUSTOM: &str = "Własna";
pub const LABEL_PALETTE_PREVIEW: &str = "Podgląd palety:";
pub const LABEL_CUSTOM_COLORS: &str = "Własne kolory:";
pub const SHADE_WHITE: &str = "Najjaśniejszy";
pub const SHADE_LIGHT_GRAY: &str = "Jasnoszary";
pub const SHADE_DARK_GRAY: &str = "Ciemnoszary";
pub const SHADE_BLACK: &str = "Najciemniejszy";

// CGB
pub const GROUP_CGB: &str = "Game Boy Color";
pub const LABEL_CGB_COLOR_CORRECTION: &str = "Korekcja kolorów CGB";
pub const TOOLTIP_CGB_COLOR_CORRECTION: &str =
    "Emuluje profil kolorów oryginalnego ekranu ciekłokrystalicznego CGB";

// Audio
pub const GROUP_AUDIO: &str = "Dźwięk";
pub const LABEL_MASTER_VOLUME: &str = "Głośność główna";
pub const LABEL_MUTE: &str = "Wycisz";
pub const LABEL_AUDIO_CHANNELS: &str = "Aktywne kanały dźwiękowe:";
pub const LABEL_CHANNEL_1: &str = "Kanał 1 (Pulse 1 z modulacją częstotliwości)";
pub const LABEL_CHANNEL_2: &str = "Kanał 2 (Pulse 2)";
pub const LABEL_CHANNEL_3: &str = "Kanał 3 (Wave)";
pub const LABEL_CHANNEL_4: &str = "Kanał 4 (Noise)";
pub const LABEL_BUFFER_SIZE: &str = "Rozmiar bufora dźwięku";
pub const SAMPLES_UNIT: &str = "próbek";
pub const TOOLTIP_BUFFER_SIZE: &str =
    "Mniejszy bufor zmniejsza opóźnienie, większy zapobiega trzaskom";

// Emulation
pub const GROUP_EMULATION: &str = "Emulacja";
pub const LABEL_FAST_FORWARD_SPEED: &str = "Prędkość szybkiego przewijania";
pub const SPEED_2X: &str = "2x";
pub const SPEED_4X: &str = "4x";
pub const SPEED_8X: &str = "8x";

// Paths
pub const GROUP_PATHS: &str = "Ścieżki zapisu";
pub const LABEL_SAVE_DIRECTORY: &str = "Lokalizacja zapisów stanu i pamięci RAM:";
pub const SAVE_DIR_ROM: &str = "Domyślna (obok pliku ROM)";
pub const SAVE_DIR_CONFIG: &str = "W katalogu konfiguracji emulatora";
pub const LABEL_CURRENT_CONFIG_DIR: &str = "Ścieżka katalogu konfiguracji:";

// ── Tab 3: Sterowanie (Controls) ───────────────────────────────────────────────
pub const GROUP_GAMEBOY_BUTTONS: &str = "Przyciski Game Boy";
pub const GROUP_HOTKEYS: &str = "Klawisze skrótów";
pub const BTN_RESTORE_DEFAULTS: &str = "Przywróć domyślne";
pub const PRESS_A_KEY: &str = "Naciśnij klawisz...";
pub const CONFLICT_WARNING: &str =
    "Uwaga: konflikt klawiszy! Niektóre akcje mają przypisany ten sam klawisz.";
pub const TOOLTIP_REBIND: &str = "Kliknij, a następnie naciśnij dowolny klawisz, aby przypisać";

// Action names
pub const ACTION_UP: &str = "W górę";
pub const ACTION_DOWN: &str = "W dół";
pub const ACTION_LEFT: &str = "W lewo";
pub const ACTION_RIGHT: &str = "W prawo";
pub const ACTION_A: &str = "Przycisk A";
pub const ACTION_B: &str = "Przycisk B";
pub const ACTION_START: &str = "Start";
pub const ACTION_SELECT: &str = "Select";

// Hotkey names
pub const HOTKEY_FAST_FORWARD: &str = "Szybkie przewijanie";
pub const HOTKEY_PAUSE: &str = "Pauza";
pub const HOTKEY_RESET: &str = "Reset konsoli";
pub const HOTKEY_SAVE_STATE: &str = "Zapisz stan (Save State)";
pub const HOTKEY_LOAD_STATE: &str = "Wczytaj stan (Load State)";
pub const HOTKEY_SCREENSHOT: &str = "Zrzut ekranu";
pub const HOTKEY_FULLSCREEN: &str = "Pełny ekran";

// ── Dialogs & Filters (rfd) ────────────────────────────────────────────────────
pub const DIALOG_OPEN_ROM_TITLE: &str = "Wybierz plik ROM Game Boy";
pub const DIALOG_CHOOSE_LIBRARY_TITLE: &str = "Wybierz katalog biblioteki gier";
pub const FILTER_ROMS_NAME: &str = "Pliki Game Boy (*.gb, *.gbc)";
pub const FILTER_ALL_FILES_NAME: &str = "Wszystkie pliki (*.*)";

// ── In-game Messages & Overlay ─────────────────────────────────────────────────
pub const MSG_STATE_SAVED: &str = "Stan gry został pomyślnie zapisany";
pub const MSG_STATE_LOADED: &str = "Stan gry został wczytany";
pub const MSG_STATE_SAVE_FAILED: &str = "Błąd: nie udało się zapisać stanu gry";
pub const MSG_STATE_LOAD_FAILED: &str = "Błąd: nie udało się wczytać stanu gry";
pub const MSG_PAUSED: &str = "PAUZA";
pub const MSG_RESUMED: &str = "WZNOWIONO";
pub const MSG_SCREENSHOT_SAVED: &str = "Zrzut ekranu został zapisany";
pub const MSG_SCREENSHOT_FAILED: &str = "Błąd zapisu zrzutu ekranu";
pub const MSG_FAST_FORWARD_ON: &str = "Szybkie przewijanie: WŁĄCZONE";
pub const MSG_FAST_FORWARD_OFF: &str = "Szybkie przewijanie: WYŁĄCZONE";
pub const MSG_RESET: &str = "Zresetowano konsolę";

// ── Context Menu (Emulator Window) ─────────────────────────────────────────────
pub const MENU_PAUSE: &str = "Pauza";
pub const MENU_RESUME: &str = "Wznów";
pub const MENU_RESET: &str = "Zresetuj";
pub const MENU_BACK_TO_LAUNCHER: &str = "Powrót do launchera";
pub const MENU_CLOSE: &str = "Zamknij";

// ── Error Messages ─────────────────────────────────────────────────────────────
pub const ERR_LOAD_ROM_FAILED: &str = "Nie udało się odczytać pliku ROM";
pub const ERR_READ_SAVE_FAILED: &str = "Nie udało się odczytać pliku zapisu bateryjnego";
pub const ERR_WRITE_SAVE_FAILED: &str = "Nie udało się zapisać danych pamięci bateryjnej";
pub const ERR_CONFIG_LOAD_FAILED: &str =
    "Błąd wczytywania pliku konfiguracji – przywrócono domyślne";
pub const ERR_CONFIG_SAVE_FAILED: &str = "Nie udało się zapisać pliku konfiguracji";
pub const ERR_CREATE_WINDOW: &str = "Nie udało się utworzyć okna emulatora";
pub const ERR_CREATE_PIXELS: &str = "Nie udało się zainicjalizować powierzchni graficznej Pixels";

// Verification string containing all Polish diacritics in lowercase and uppercase
pub const POLISH_DIACRITICS_SAMPLE: &str =
    "Zażółć gęślą jaźń – ZAŻÓŁĆ GĘŚLĄ JAŹŃ (ą, ć, ę, ł, ń, ó, ś, ź, ż / Ą, Ć, Ę, Ł, Ń, Ó, Ś, Ź, Ż)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polish_characters_layout_in_egui() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            // Shape a galley with all Polish letters to ensure font rasterizer handles them without panic
            let text = POLISH_DIACRITICS_SAMPLE;
            let job = egui::text::LayoutJob::simple_singleline(
                text.to_string(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
            let galley = ctx.fonts(|fonts| fonts.layout_job(job));
            assert!(galley.size().x > 0.0, "Galley width must be positive");
            assert!(galley.size().y > 0.0, "Galley height must be positive");
            assert_eq!(galley.text(), text, "Text should match exactly");
        });
    }
}
