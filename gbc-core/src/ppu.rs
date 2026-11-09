//! Picture processing unit.
//!
//! The PPU advances one dot at a time, following the four LCD modes, and
//! renders background, window, and sprite pixels into a tightly packed
//! RGBA8888 framebuffer. CGB palette RAM and tile attributes are supported.

use crate::state::{SaveStateError, StateReader, StateWriter};
use crate::{FRAMEBUFFER_LEN, SCREEN_HEIGHT, SCREEN_WIDTH};

const DOTS_PER_LINE: u16 = 456;
const MODE2_END: u16 = 80;
const MODE3_END: u16 = 252;
const VISIBLE_LINES: u8 = 144;
const TOTAL_LINES: u8 = 154;

const VRAM_BANK_SIZE: usize = 0x2000;
#[cfg(test)]
const OAM_SIZE: usize = 0xA0;

const LCDC_ENABLE: u8 = 0x80;
const LCDC_WINDOW_TILE_MAP: u8 = 0x40;
const LCDC_WINDOW_ENABLE: u8 = 0x20;
const LCDC_BG_TILE_DATA: u8 = 0x10;
const LCDC_BG_TILE_MAP: u8 = 0x08;
const LCDC_OBJ_SIZE: u8 = 0x04;
const LCDC_OBJ_ENABLE: u8 = 0x02;
/// On DMG: BG enable. On CGB: BG/Window master priority enable.
const LCDC_BG_WIN_ENABLE: u8 = 0x01;

const STAT_LYC: u8 = 0x40;
const STAT_MODE2: u8 = 0x20;
const STAT_MODE1: u8 = 0x10;
const STAT_MODE0: u8 = 0x08;

/// Game-Boy-green DMG palette shades.
const WHITE_RGBA: [u8; 4] = [0xE0, 0xF8, 0xD0, 0xFF];
const DMG_RGBA: [[u8; 4]; 4] = [
    [0xE0, 0xF8, 0xD0, 0xFF],
    [0x88, 0xC0, 0x70, 0xFF],
    [0x34, 0x68, 0x56, 0xFF],
    [0x08, 0x18, 0x20, 0xFF],
];

/// LCD controller operating mode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Horizontal blank.
    #[default]
    HBlank = 0,
    /// Vertical blank.
    VBlank = 1,
    /// OAM search.
    OamSearch = 2,
    /// Pixel transfer.
    PixelTransfer = 3,
}

/// Interrupt requests produced by the PPU since the last poll.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PpuInterrupts {
    pub(crate) vblank: bool,
    pub(crate) stat: bool,
}

#[derive(Debug, Clone, Copy)]
struct SpritePixel {
    rgba: u32,
    /// OAM attribute bit 7: sprite is behind non-zero BG/Window colors.
    bg_over_obj: bool,
}

/// The LCD controller and its output buffer.
#[derive(Debug)]
pub struct Ppu {
    framebuffer: Vec<u8>,
    lcdc: u8,
    stat: u8,
    scy: u8,
    scx: u8,
    ly: u8,
    lyc: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    wy: u8,
    wx: u8,
    mode: Mode,
    dot: u16,
    /// CGB: internal window line counter — only incremented when window was
    /// actually rendered on a scanline.
    window_line: u8,
    hblank_entered: bool,
    cgb: bool,
    bg_palette: [u8; 64],
    obj_palette: [u8; 64],
    bgpi: u8,
    obpi: u8,
    opri: u8,
    stat_line: bool,
    vblank_irq: bool,
    stat_irq: bool,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Ppu {
    /// Creates a PPU in DMG or CGB mode.
    pub fn new(cgb: bool) -> Self {
        let mut framebuffer = vec![0; FRAMEBUFFER_LEN];
        for pixel in framebuffer.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&WHITE_RGBA);
        }

        // CGB boot ROM initializes all palette entries to white (0x7FFF).
        let bg_palette = [0xFF; 64];
        let obj_palette = [0xFF; 64];

        Self {
            framebuffer,
            lcdc: 0,
            stat: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            mode: Mode::HBlank,
            dot: 0,
            window_line: 0,
            hblank_entered: false,
            cgb,
            bg_palette,
            obj_palette,
            bgpi: 0,
            obpi: 0,
            opri: 0,
            stat_line: false,
            vblank_irq: false,
            stat_irq: false,
        }
    }

    /// The current frame as tightly packed RGBA8888 bytes
    /// (`SCREEN_WIDTH * SCREEN_HEIGHT * 4`).
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// The LCD dimensions in pixels.
    pub const fn dimensions() -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_u8(self.lcdc);
        w.write_u8(self.stat);
        w.write_u8(self.scy);
        w.write_u8(self.scx);
        w.write_u8(self.ly);
        w.write_u8(self.lyc);
        w.write_u8(self.bgp);
        w.write_u8(self.obp0);
        w.write_u8(self.obp1);
        w.write_u8(self.wy);
        w.write_u8(self.wx);
        w.write_u8(self.mode as u8);
        w.write_u16(self.dot);
        w.write_u8(self.window_line);
        w.write_bool(self.hblank_entered);
        w.write_bool(self.cgb);
        w.write_raw(&self.bg_palette);
        w.write_raw(&self.obj_palette);
        w.write_u8(self.bgpi);
        w.write_u8(self.obpi);
        w.write_u8(self.opri);
        w.write_bool(self.stat_line);
        w.write_bool(self.vblank_irq);
        w.write_bool(self.stat_irq);
        w.write_bytes(&self.framebuffer);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.lcdc = r.read_u8()?;
        self.stat = r.read_u8()?;
        self.scy = r.read_u8()?;
        self.scx = r.read_u8()?;
        self.ly = r.read_u8()?;
        self.lyc = r.read_u8()?;
        self.bgp = r.read_u8()?;
        self.obp0 = r.read_u8()?;
        self.obp1 = r.read_u8()?;
        self.wy = r.read_u8()?;
        self.wx = r.read_u8()?;
        let mode_raw = r.read_u8()?;
        self.mode = match mode_raw {
            0 => Mode::HBlank,
            1 => Mode::VBlank,
            2 => Mode::OamSearch,
            3 => Mode::PixelTransfer,
            _ => return Err(SaveStateError::InvalidData("invalid ppu mode")),
        };
        self.dot = r.read_u16()?;
        self.window_line = r.read_u8()?;
        self.hblank_entered = r.read_bool()?;
        self.cgb = r.read_bool()?;
        self.bg_palette.copy_from_slice(r.read_raw(64)?);
        self.obj_palette.copy_from_slice(r.read_raw(64)?);
        self.bgpi = r.read_u8()?;
        self.obpi = r.read_u8()?;
        self.opri = r.read_u8()?;
        self.stat_line = r.read_bool()?;
        self.vblank_irq = r.read_bool()?;
        self.stat_irq = r.read_bool()?;
        let fb = r.read_bytes()?;
        if fb.len() != self.framebuffer.len() {
            return Err(SaveStateError::InvalidData("framebuffer size mismatch"));
        }
        self.framebuffer.copy_from_slice(fb);
        Ok(())
    }

    pub(crate) fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => self.read_stat(),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF68 if self.cgb => self.bgpi | 0x40,
            0xFF69 if self.cgb && self.palette_access_allowed() => {
                self.bg_palette[(self.bgpi & 0x3F) as usize]
            }
            0xFF6A if self.cgb => self.obpi | 0x40,
            0xFF6B if self.cgb && self.palette_access_allowed() => {
                self.obj_palette[(self.obpi & 0x3F) as usize]
            }
            0xFF6C if self.cgb => self.opri | 0xFE,
            _ => 0xFF,
        }
    }

    pub(crate) fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF40 => self.write_lcdc(value),
            0xFF41 => {
                self.stat = value & 0x78;
                self.refresh_stat();
            }
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => self.reset_ly(),
            0xFF45 => {
                self.lyc = value;
                self.refresh_stat();
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            0xFF68 if self.cgb => self.bgpi = value & 0xBF,
            0xFF69 if self.cgb => self.write_bgpd(value),
            0xFF6A if self.cgb => self.obpi = value & 0xBF,
            0xFF6B if self.cgb => self.write_obpd(value),
            0xFF6C if self.cgb => self.opri = value & 0x01,
            _ => {}
        }
    }

    /// Advances the PPU by `cycles` dots.
    pub(crate) fn tick(&mut self, cycles: u32, vram: &[u8], oam: &[u8]) {
        if self.lcdc & LCDC_ENABLE == 0 {
            return;
        }

        for _ in 0..cycles {
            self.dot += 1;

            match self.mode {
                Mode::OamSearch if self.dot >= MODE2_END => {
                    self.mode = Mode::PixelTransfer;
                    self.refresh_stat();
                }
                Mode::PixelTransfer if self.dot >= MODE3_END => {
                    self.render_line(vram, oam);
                    self.mode = Mode::HBlank;
                    self.hblank_entered = true;
                    self.refresh_stat();
                }
                Mode::HBlank | Mode::VBlank if self.dot >= DOTS_PER_LINE => {
                    self.advance_line();
                }
                _ => {}
            }
        }
    }

    /// Returns and clears pending PPU interrupt requests.
    pub(crate) fn take_interrupts(&mut self) -> PpuInterrupts {
        let interrupts = PpuInterrupts {
            vblank: self.vblank_irq,
            stat: self.stat_irq,
        };
        self.vblank_irq = false;
        self.stat_irq = false;
        interrupts
    }

    /// Returns whether Mode 0 (H-Blank) was entered and clears the flag.
    pub(crate) fn take_hblank_entered(&mut self) -> bool {
        let entered = self.hblank_entered;
        self.hblank_entered = false;
        entered
    }

    fn read_stat(&self) -> u8 {
        let coincidence = if self.ly == self.lyc { 0x04 } else { 0 };
        0x80 | (self.stat & 0x78) | coincidence | self.mode as u8
    }

    fn write_lcdc(&mut self, value: u8) {
        let was_enabled = self.lcdc & LCDC_ENABLE != 0;
        let now_enabled = value & LCDC_ENABLE != 0;

        self.lcdc = value;

        if was_enabled && !now_enabled {
            self.ly = 0;
            self.dot = 0;
            self.window_line = 0;
            self.mode = Mode::HBlank;
            self.hblank_entered = false;
            // Blank the framebuffer when the LCD is turned off.
            for pixel in self.framebuffer.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&WHITE_RGBA);
            }
        } else if !was_enabled && now_enabled {
            self.ly = 0;
            self.dot = 0;
            self.window_line = 0;
            self.mode = Mode::OamSearch;
        }

        self.refresh_stat();
    }

    fn reset_ly(&mut self) {
        self.ly = 0;
        self.dot = 0;
        self.window_line = 0;
        self.mode = if self.lcdc & LCDC_ENABLE != 0 {
            Mode::OamSearch
        } else {
            Mode::HBlank
        };
        self.refresh_stat();
    }

    fn advance_line(&mut self) {
        self.dot = 0;
        self.ly = self.ly.wrapping_add(1);

        if self.ly >= TOTAL_LINES {
            self.ly = 0;
            self.window_line = 0;
            self.mode = Mode::OamSearch;
        } else if self.ly >= VISIBLE_LINES {
            if self.ly == VISIBLE_LINES {
                self.vblank_irq = true;
            }
            self.mode = Mode::VBlank;
        } else {
            self.mode = Mode::OamSearch;
        }

        self.refresh_stat();
    }

    fn refresh_stat(&mut self) {
        let enabled = self.lcdc & LCDC_ENABLE != 0;
        let line = (self.stat & STAT_LYC != 0 && self.ly == self.lyc)
            || (enabled && self.stat & STAT_MODE2 != 0 && self.mode == Mode::OamSearch)
            || (enabled && self.stat & STAT_MODE1 != 0 && self.mode == Mode::VBlank)
            || (enabled && self.stat & STAT_MODE0 != 0 && self.mode == Mode::HBlank);

        if line && !self.stat_line {
            self.stat_irq = true;
        }

        self.stat_line = line;
    }

    fn palette_access_allowed(&self) -> bool {
        self.mode != Mode::PixelTransfer
    }

    fn write_bgpd(&mut self, value: u8) {
        let index = (self.bgpi & 0x3F) as usize;
        if self.palette_access_allowed() {
            self.bg_palette[index] = value;
        }
        if self.bgpi & 0x80 != 0 {
            self.bgpi = 0x80 | ((index as u8).wrapping_add(1) & 0x3F);
        }
    }

    fn write_obpd(&mut self, value: u8) {
        let index = (self.obpi & 0x3F) as usize;
        if self.palette_access_allowed() {
            self.obj_palette[index] = value;
        }
        if self.obpi & 0x80 != 0 {
            self.obpi = 0x80 | ((index as u8).wrapping_add(1) & 0x3F);
        }
    }

    fn render_line(&mut self, vram: &[u8], oam: &[u8]) {
        let ly = self.ly;

        // LCDC.0 meaning differs between DMG and CGB:
        //   DMG: 0 = BG/Window disabled (show white), 1 = enabled.
        //   CGB: 0 = master priority off (sprites always win), 1 = normal.
        let bg_win_enable = self.lcdc & LCDC_BG_WIN_ENABLE != 0;

        // Per-pixel BG color index and tile-attribute priority flag.
        let mut bg_index = [0u8; SCREEN_WIDTH];
        let mut bg_priority = [false; SCREEN_WIDTH]; // tile attr bit 7
        let mut colors = [rgba_to_u32(WHITE_RGBA); SCREEN_WIDTH];
        let mut window_rendered = false;

        // ── Background & Window ──────────────────────────────────────────────
        // CGB: render BG even when bg_win_enable=0 (sprites just always win).
        // DMG: skip BG when bg_win_enable=0 (white background).
        let render_bg = bg_win_enable || self.cgb;

        if render_bg {
            let bg_map = if self.lcdc & LCDC_BG_TILE_MAP != 0 {
                0x1C00usize
            } else {
                0x1800
            };
            let win_map = if self.lcdc & LCDC_WINDOW_TILE_MAP != 0 {
                0x1C00usize
            } else {
                0x1800
            };

            // Window is active on this line when LCDC.5=1, WX valid, and LY >= WY.
            let win_on = bg_win_enable
                && self.lcdc & LCDC_WINDOW_ENABLE != 0
                && self.wx < 167
                && ly >= self.wy;
            let win_x0 = self.wx.saturating_sub(7) as usize;

            for x in 0..SCREEN_WIDTH {
                let use_win = win_on && x >= win_x0;
                let (map, tile_col, tile_row, fine_x, fine_y) = if use_win {
                    window_rendered = true;
                    let wx = x - win_x0;
                    let wy = self.window_line as usize;
                    (win_map, (wx >> 3) & 0x1F, (wy >> 3) & 0x1F, wx & 7, wy & 7)
                } else {
                    let bx = (x + self.scx as usize) & 0xFF;
                    let by = (ly as usize + self.scy as usize) & 0xFF;
                    (bg_map, (bx >> 3) & 0x1F, (by >> 3) & 0x1F, bx & 7, by & 7)
                };

                let map_off = map + tile_row * 32 + tile_col;
                let tile_idx = vram[map_off];
                // CGB: tile attributes live in VRAM bank 1 at the same offset.
                let attr = if self.cgb {
                    vram[VRAM_BANK_SIZE + map_off]
                } else {
                    0
                };
                let bank = if self.cgb && attr & 0x08 != 0 {
                    VRAM_BANK_SIZE
                } else {
                    0
                };

                // Horizontal/vertical flip from attribute bits 5 & 6.
                let px = if attr & 0x20 != 0 { 7 - fine_x } else { fine_x };
                let py = if attr & 0x40 != 0 { 7 - fine_y } else { fine_y };

                let tile_addr = self.tile_data_addr(tile_idx) + py * 2;
                let lo = vram[bank + tile_addr];
                let hi = vram[bank + tile_addr + 1];
                let bit = 7 - px;
                let color = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);

                bg_index[x] = color;
                // Tile attribute bit 7 gives BG priority (only active in CGB
                // mode when the master priority flag LCDC.0 is also set).
                bg_priority[x] = self.cgb && bg_win_enable && attr & 0x80 != 0;

                colors[x] = rgba_to_u32(if self.cgb {
                    cgb_color(&self.bg_palette, (attr & 0x07) as usize, color)
                } else {
                    dmg_color(self.bgp, color)
                });
            }
        }

        if window_rendered {
            self.window_line = self.window_line.wrapping_add(1);
        }

        // ── Sprites ──────────────────────────────────────────────────────────
        let mut sprite_pixels: [Option<SpritePixel>; SCREEN_WIDTH] = [None; SCREEN_WIDTH];

        if self.lcdc & LCDC_OBJ_ENABLE != 0 {
            let height: i16 = if self.lcdc & LCDC_OBJ_SIZE != 0 {
                16
            } else {
                8
            };

            // Collect up to 10 sprites that intersect this scanline.
            let mut selected: Vec<usize> = Vec::with_capacity(10);
            for idx in 0..40usize {
                let base = idx * 4;
                let sy = oam[base] as i16 - 16;
                let row = ly as i16 - sy;
                if (0..height).contains(&row) {
                    selected.push(base);
                    if selected.len() == 10 {
                        break;
                    }
                }
            }

            // Priority ordering for rendering (last drawn wins):
            //   DMG: lower X-coord wins, ties by lower OAM index.
            //        → sort descending so highest X/index drawn first (can be
            //          overwritten by lower X/index drawn later).
            //   CGB: lower OAM index wins.
            //        → reverse so index 0 is drawn last (overwrites others).
            if !self.cgb {
                selected.sort_unstable_by(|&a, &b| {
                    let ax = oam[a + 1];
                    let bx = oam[b + 1];
                    bx.cmp(&ax).then(b.cmp(&a))
                });
            } else {
                selected.reverse();
            }

            for base in selected {
                let sx = oam[base + 1] as i16 - 8;
                let sy = oam[base] as i16 - 16;
                let attr = oam[base + 3];
                let row = ly as i16 - sy;

                // Apply Y-flip. For 8×16, top/bottom half selection happens
                // on the flipped row so both halves flip correctly.
                let flipped_row = if attr & 0x40 != 0 {
                    height - 1 - row
                } else {
                    row
                };

                let mut tile = oam[base + 2];
                let row_in_tile = if height == 16 {
                    tile &= 0xFE; // bit 0 ignored for 8×16
                    tile = tile.wrapping_add((flipped_row / 8) as u8);
                    flipped_row % 8
                } else {
                    flipped_row
                };

                let bank = if self.cgb && attr & 0x08 != 0 {
                    VRAM_BANK_SIZE
                } else {
                    0
                };
                let tile_addr = tile as usize * 16 + row_in_tile as usize * 2;
                let lo = vram[bank + tile_addr];
                let hi = vram[bank + tile_addr + 1];

                for col in 0..8i16 {
                    let screen_x = sx + col;
                    if !(0..SCREEN_WIDTH as i16).contains(&screen_x) {
                        continue;
                    }
                    let screen_x = screen_x as usize;

                    let px = if attr & 0x20 != 0 { 7 - col } else { col };
                    let bit = 7 - px as u8;
                    let color = ((lo >> bit) & 1) | (((hi >> bit) & 1) << 1);

                    if color == 0 {
                        continue; // transparent
                    }

                    let rgba = if self.cgb {
                        cgb_color(&self.obj_palette, (attr & 0x07) as usize, color)
                    } else {
                        let pal = if attr & 0x10 != 0 {
                            self.obp1
                        } else {
                            self.obp0
                        };
                        dmg_color(pal, color)
                    };

                    // Overwrite — higher-priority sprites are drawn last.
                    sprite_pixels[screen_x] = Some(SpritePixel {
                        rgba: rgba_to_u32(rgba),
                        bg_over_obj: attr & 0x80 != 0,
                    });
                }
            }
        }

        // ── Compositor ───────────────────────────────────────────────────────
        for x in 0..SCREEN_WIDTH {
            let Some(sprite) = sprite_pixels[x] else {
                continue;
            };

            // When does the background pixel win?
            //   • LCDC.0=0 on DMG: bg disabled → sprite always wins.
            //   • LCDC.0=0 on CGB: master priority off → sprite always wins.
            //   • bg color index 0 → transparent BG → sprite wins.
            //   • CGB: bg tile attr.7 set (and master priority on) → bg wins.
            //   • sprite attr.7 set → bg wins (if bg non-transparent).
            let bg_wins = render_bg
                && bg_win_enable
                && bg_index[x] != 0
                && if self.cgb {
                    bg_priority[x] || sprite.bg_over_obj
                } else {
                    sprite.bg_over_obj
                };

            if !bg_wins {
                colors[x] = sprite.rgba;
            }
        }

        // ── Write scanline to framebuffer ────────────────────────────────────
        let line_start = ly as usize * SCREEN_WIDTH * 4;
        for (x, &color) in colors.iter().enumerate() {
            let off = line_start + x * 4;
            self.framebuffer[off..off + 4].copy_from_slice(&color.to_le_bytes());
        }
    }

    /// VRAM offset for the first byte of tile `tile`'s pixel data.
    fn tile_data_addr(&self, tile: u8) -> usize {
        if self.lcdc & LCDC_BG_TILE_DATA != 0 {
            tile as usize * 16
        } else {
            (0x1000i32 + i32::from(tile as i8) * 16) as usize
        }
    }
}

fn rgba_to_u32(rgba: [u8; 4]) -> u32 {
    u32::from_le_bytes(rgba)
}

fn dmg_color(palette: u8, color: u8) -> [u8; 4] {
    let shade = (palette >> (color * 2)) & 0x03;
    DMG_RGBA[shade as usize]
}

/// Converts a 15-bit GBC colour to RGBA8888.
/// Each 5-bit channel is expanded to 8 bits: `v5 << 3 | v5 >> 2`.
fn cgb_color(palette: &[u8; 64], palette_index: usize, color: u8) -> [u8; 4] {
    let off = palette_index * 8 + color as usize * 2;
    let raw = u16::from_le_bytes([palette[off], palette[off + 1]]) & 0x7FFF;
    let r5 = (raw & 0x1F) as u8;
    let g5 = ((raw >> 5) & 0x1F) as u8;
    let b5 = ((raw >> 10) & 0x1F) as u8;
    [
        (r5 << 3) | (r5 >> 2),
        (g5 << 3) | (g5 >> 2),
        (b5 << 3) | (b5 >> 2),
        0xFF,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vram() -> Vec<u8> {
        vec![0; VRAM_BANK_SIZE * 2]
    }

    fn oam() -> [u8; OAM_SIZE] {
        [0; OAM_SIZE]
    }

    #[test]
    fn mode_timing_and_ly_advance() {
        let mut ppu = Ppu::new(false);
        let vram = vram();
        let oam = oam();

        ppu.write(0xFF40, 0x80);
        assert_eq!(ppu.read(0xFF41) & 0x03, Mode::OamSearch as u8);

        ppu.tick(79, &vram, &oam);
        assert_eq!(ppu.read(0xFF41) & 0x03, Mode::OamSearch as u8);

        ppu.tick(1, &vram, &oam);
        assert_eq!(ppu.read(0xFF41) & 0x03, Mode::PixelTransfer as u8);

        ppu.tick(172, &vram, &oam);
        assert_eq!(ppu.read(0xFF41) & 0x03, Mode::HBlank as u8);

        ppu.tick(204, &vram, &oam);
        assert_eq!(ppu.read(0xFF44), 1);
        assert_eq!(ppu.read(0xFF41) & 0x03, Mode::OamSearch as u8);
    }

    #[test]
    fn renders_background_tile() {
        let mut ppu = Ppu::new(false);
        let mut vram = vram();
        let oam = oam();

        vram[0] = 0xFF;
        vram[1] = 0x00;
        ppu.write(0xFF40, 0x91);
        ppu.write(0xFF47, 0xE4);
        ppu.tick(MODE3_END as u32, &vram, &oam);

        assert_eq!(&ppu.framebuffer()[0..4], &DMG_RGBA[1]);
    }

    #[test]
    fn renders_window_over_background() {
        let mut ppu = Ppu::new(false);
        let mut vram = vram();
        let oam = oam();

        vram[0] = 0xFF;
        vram[1] = 0x00;
        vram[16] = 0x00;
        vram[17] = 0xFF;
        vram[0x1C00] = 1;

        ppu.write(0xFF40, 0xF1);
        ppu.write(0xFF47, 0xE4);
        ppu.write(0xFF4A, 0); // WY = 0
        ppu.write(0xFF4B, 7); // WX = 7 → starts at col 0
                              // Tick far enough to reach line 0 (WY=0 triggers immediately).
        ppu.tick(MODE3_END as u32, &vram, &oam);

        assert_eq!(&ppu.framebuffer()[0..4], &DMG_RGBA[2]);
    }

    #[test]
    fn renders_sprite_tile() {
        let mut ppu = Ppu::new(false);
        let mut vram = vram();
        let mut oam = oam();

        vram[0] = 0xFF;
        vram[1] = 0x00;
        oam[0] = 16;
        oam[1] = 8;
        oam[2] = 0;
        oam[3] = 0;

        ppu.write(0xFF40, 0x82);
        ppu.write(0xFF48, 0xE4);
        ppu.tick(MODE3_END as u32, &vram, &oam);

        assert_eq!(&ppu.framebuffer()[0..4], &DMG_RGBA[1]);
    }

    #[test]
    fn cgb_tile_attributes_select_vram_bank_and_palette() {
        let mut ppu = Ppu::new(true);
        let mut vram = vram();
        let oam = oam();

        vram[0] = 0xFF;
        vram[1] = 0x00;
        vram[VRAM_BANK_SIZE] = 0x00;
        vram[VRAM_BANK_SIZE + 1] = 0xFF;
        vram[VRAM_BANK_SIZE + 0x1800] = 0x08 | 0x01; // bank 1, palette 1

        // Palette 1 color 2 = pure green (0x03E0 → lo=0xE0, hi=0x03).
        ppu.write(0xFF68, 0x80 | (8 + 2 * 2));
        ppu.write(0xFF69, 0xE0);
        ppu.write(0xFF69, 0x03);

        ppu.write(0xFF40, 0x91);
        ppu.tick(MODE3_END as u32, &vram, &oam);

        assert_eq!(&ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn lyc_and_stat_interrupts_are_edge_triggered() {
        let mut ppu = Ppu::new(false);
        let vram = vram();
        let oam = oam();

        ppu.write(0xFF40, 0x80);
        ppu.write(0xFF45, 1);
        ppu.write(0xFF41, STAT_LYC);

        assert!(!ppu.take_interrupts().stat);

        ppu.tick(DOTS_PER_LINE as u32, &vram, &oam);
        assert!(ppu.take_interrupts().stat);
        assert_eq!(ppu.read(0xFF44), 1);
    }

    #[test]
    fn entering_vblank_requests_vblank_interrupt() {
        let mut ppu = Ppu::new(false);
        let vram = vram();
        let oam = oam();

        ppu.write(0xFF40, 0x80);
        ppu.tick(DOTS_PER_LINE as u32 * VISIBLE_LINES as u32, &vram, &oam);

        assert_eq!(ppu.read(0xFF44), VISIBLE_LINES);
        assert!(ppu.take_interrupts().vblank);
    }

    #[test]
    fn cgb_lcdc0_zero_sprites_always_win_over_bg_priority() {
        // With LCDC.0=0 in CGB mode, sprite appears above BG even when tile
        // attribute bit 7 (BG priority) is set.
        let mut ppu = Ppu::new(true);
        let mut vram = vram();
        let mut oam = oam();

        // BG tile 0 row 0: color index 1 everywhere.
        vram[0] = 0xFF;
        vram[1] = 0x00;
        // CGB tile attr for tile [0,0]: BG priority bit set.
        vram[VRAM_BANK_SIZE + 0x1800] = 0x80;

        // Sprite tile 1 row 0: color index 1 everywhere.
        vram[16] = 0xFF;
        vram[17] = 0x00;
        oam[0] = 16; // Y
        oam[1] = 8; // X → screen col 0
        oam[2] = 1; // tile 1
        oam[3] = 0; // no flags

        // Set OBJ palette 0 color 1 (offset 2..3) to pure red (0x001F → lo=0x1F, hi=0x00).
        ppu.write(0xFF6A, 0x80 | 2);
        ppu.write(0xFF6B, 0x1F);
        ppu.write(0xFF6B, 0x00);

        // LCDC: LCD on + OBJ enable + BG data $8000. LCDC.0=0 → master priority off.
        // 0x92 = 1001_0010 (LCD | BG_DATA | OBJ). No BG_WIN_ENABLE bit.
        ppu.write(0xFF40, 0x92);
        ppu.tick(MODE3_END as u32, &vram, &oam);

        let pixel = &ppu.framebuffer()[0..4];
        // Sprite (red) should win despite bg tile priority bit.
        assert_eq!(pixel[0], 0xFF, "red channel: sprite should win");
        assert_eq!(pixel[1], 0x00, "green: should be 0");
        assert_eq!(pixel[2], 0x00, "blue: should be 0");
    }

    #[test]
    fn dmg_sprite_x_priority_lower_x_wins() {
        // Two sprites both covering screen column 7.
        // Sprite A: X=8 (covers screen col 0..=7), lower X → higher priority.
        // Sprite B: X=15 (covers screen col 7..=14), higher X → lower priority.
        // B is first in OAM. Lower X (Sprite A) should win.
        let mut ppu = Ppu::new(false);
        let mut vram = vram();
        let mut oam = oam();

        // Tile 0: color index 1 (sprite A).
        vram[0] = 0xFF;
        vram[1] = 0x00;
        // Tile 1: color index 3 (sprite B).
        vram[16] = 0xFF;
        vram[17] = 0xFF;

        // B first in OAM (would win if only OAM-index priority used).
        oam[0] = 16;
        oam[1] = 15;
        oam[2] = 1;
        oam[3] = 0; // B at X=15
        oam[4] = 16;
        oam[5] = 8;
        oam[6] = 0;
        oam[7] = 0; // A at X=8

        ppu.write(0xFF40, 0x82);
        ppu.write(0xFF48, 0xE4); // identity palette

        ppu.tick(MODE3_END as u32, &vram, &oam);

        // Screen col 7 is covered by both sprites. A (X=8) has lower X → higher priority → should win.
        // Tile 0 color 1 → DMG_RGBA[1] = [0x88, 0xC0, 0x70, 0xFF].
        let pixel = &ppu.framebuffer()[7 * 4..7 * 4 + 4];
        assert_eq!(pixel, &DMG_RGBA[1], "lower-X sprite should win at col 7");
    }
}
