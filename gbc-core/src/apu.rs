//! Audio processing unit (APU).
//!
//! Emulates the four Game Boy audio channels:
//! 1. Pulse with frequency sweep & volume envelope
//! 2. Pulse with volume envelope
//! 3. Wave output (32 4-bit samples from Wave RAM)
//! 4. Noise with LFSR & volume envelope
//!
//! Driven by a 512 Hz frame sequencer clocked from the master oscillator.
//! Produces interleaved stereo `f32` samples at 48,000 Hz.

use crate::state::{SaveStateError, StateReader, StateWriter};
use crate::{AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

const CLOCK_SPEED: u32 = 4_194_304;

// Duty cycles: 12.5%, 25%, 50%, 75%
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 0, 0, 1],
    [1, 0, 0, 0, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 0],
];

// Noise channel divisor table (code 0..7)
const NOISE_DIVISORS: [u32; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

/// Square channel (Channels 1 and 2).
#[derive(Debug, Clone)]
pub struct PulseChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub nr1: u8, // Duty (bits 6-7), Length load (bits 0-5)
    pub nr2: u8, // Starting volume (4-7), Envelope dir (3), Period (0-2)
    pub nr3: u8, // Frequency low 8 bits
    pub nr4: u8, // Trigger (7), Length enable (6), Frequency high 3 bits (0-2)

    // Sweep (Channel 1 only)
    pub has_sweep: bool,
    pub nr0: u8, // Sweep period (4-6), Negate (3), Shift (0-2)
    pub sweep_timer: u8,
    pub sweep_shadow_freq: u16,
    pub sweep_enabled: bool,

    // Frequency timer
    pub timer: u32,
    pub duty_step: usize,

    // Length counter
    pub length_counter: u8,

    // Envelope
    pub volume: u8,
    pub envelope_timer: u8,
}

impl PulseChannel {
    pub fn new(has_sweep: bool) -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            nr1: 0,
            nr2: 0,
            nr3: 0,
            nr4: 0,
            has_sweep,
            nr0: 0,
            sweep_timer: 0,
            sweep_shadow_freq: 0,
            sweep_enabled: false,
            timer: 0,
            duty_step: 0,
            length_counter: 0,
            volume: 0,
            envelope_timer: 0,
        }
    }

    pub fn frequency(&self) -> u16 {
        u16::from(self.nr3) | (u16::from(self.nr4 & 0x07) << 8)
    }

    pub fn set_frequency(&mut self, freq: u16) {
        self.nr3 = (freq & 0xFF) as u8;
        self.nr4 = (self.nr4 & !0x07) | (((freq >> 8) & 0x07) as u8);
    }

    pub fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }

        self.timer = self.period();
        self.volume = self.nr2 >> 4;
        self.envelope_timer = self.nr2 & 0x07;

        if self.has_sweep {
            self.sweep_shadow_freq = self.frequency();
            let sweep_period = (self.nr0 >> 4) & 0x07;
            let sweep_shift = self.nr0 & 0x07;
            self.sweep_timer = if sweep_period == 0 { 8 } else { sweep_period };
            self.sweep_enabled = sweep_period > 0 || sweep_shift > 0;
            if sweep_shift > 0 {
                let _ = self.calculate_sweep_freq();
            }
        }
    }

    pub fn period(&self) -> u32 {
        (2048 - u32::from(self.frequency())) * 4
    }

    pub fn clock_timer(&mut self, cycles: u32) {
        if self.timer <= cycles {
            let p = self.period();
            let rem = cycles - self.timer;
            self.duty_step = (self.duty_step + 1 + (rem / p) as usize) % 8;
            self.timer = p - (rem % p);
        } else {
            self.timer -= cycles;
        }
    }

    pub fn clock_length(&mut self) {
        if (self.nr4 & 0x40) != 0 && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn clock_envelope(&mut self) {
        let period = self.nr2 & 0x07;
        if period == 0 {
            return;
        }

        if self.envelope_timer > 0 {
            self.envelope_timer -= 1;
        }

        if self.envelope_timer == 0 {
            self.envelope_timer = period;
            let direction_up = (self.nr2 & 0x08) != 0;
            if direction_up && self.volume < 15 {
                self.volume += 1;
            } else if !direction_up && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    fn calculate_sweep_freq(&mut self) -> Option<u16> {
        let shift = self.nr0 & 0x07;
        let delta = self.sweep_shadow_freq >> shift;
        let negate = (self.nr0 & 0x08) != 0;

        let new_freq = if negate {
            self.sweep_shadow_freq.checked_sub(delta)?
        } else {
            self.sweep_shadow_freq.checked_add(delta)?
        };

        if new_freq > 2047 {
            self.enabled = false;
            None
        } else {
            Some(new_freq)
        }
    }

    pub fn clock_sweep(&mut self) {
        if !self.has_sweep || !self.sweep_enabled {
            return;
        }

        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }

        if self.sweep_timer == 0 {
            let sweep_period = (self.nr0 >> 4) & 0x07;
            self.sweep_timer = if sweep_period == 0 { 8 } else { sweep_period };

            if sweep_period > 0 {
                if let Some(new_freq) = self.calculate_sweep_freq() {
                    let shift = self.nr0 & 0x07;
                    if shift > 0 {
                        self.sweep_shadow_freq = new_freq;
                        self.set_frequency(new_freq);
                        // Overflow check again
                        let _ = self.calculate_sweep_freq();
                    }
                }
            }
        }
    }

    pub fn sample(&self) -> u8 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }

        let duty = (self.nr1 >> 6) as usize;
        let high = DUTY_TABLE[duty][self.duty_step];
        if high != 0 {
            self.volume
        } else {
            0
        }
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_bool(self.enabled);
        w.write_bool(self.dac_enabled);
        w.write_u8(self.nr1);
        w.write_u8(self.nr2);
        w.write_u8(self.nr3);
        w.write_u8(self.nr4);
        w.write_bool(self.has_sweep);
        w.write_u8(self.nr0);
        w.write_u8(self.sweep_timer);
        w.write_u16(self.sweep_shadow_freq);
        w.write_bool(self.sweep_enabled);
        w.write_u32(self.timer);
        w.write_u32(self.duty_step as u32);
        w.write_u8(self.length_counter);
        w.write_u8(self.volume);
        w.write_u8(self.envelope_timer);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.enabled = r.read_bool()?;
        self.dac_enabled = r.read_bool()?;
        self.nr1 = r.read_u8()?;
        self.nr2 = r.read_u8()?;
        self.nr3 = r.read_u8()?;
        self.nr4 = r.read_u8()?;
        self.has_sweep = r.read_bool()?;
        self.nr0 = r.read_u8()?;
        self.sweep_timer = r.read_u8()?;
        self.sweep_shadow_freq = r.read_u16()?;
        self.sweep_enabled = r.read_bool()?;
        self.timer = r.read_u32()?;
        self.duty_step = r.read_u32()? as usize;
        self.length_counter = r.read_u8()?;
        self.volume = r.read_u8()?;
        self.envelope_timer = r.read_u8()?;
        Ok(())
    }
}

/// Wave output channel (Channel 3).
#[derive(Debug, Clone, Default)]
pub struct WaveChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub nr0: u8, // Sound on/off (bit 7)
    pub nr1: u8, // Length load
    pub nr2: u8, // Volume code (bits 5-6)
    pub nr3: u8, // Frequency low 8 bits
    pub nr4: u8, // Trigger (7), Length enable (6), Frequency high 3 bits (0-2)

    pub timer: u32,
    pub position: usize, // 0..32 nibble index
    pub length_counter: u16,
    pub wave_ram: [u8; 16],
}

impl WaveChannel {
    pub fn frequency(&self) -> u16 {
        u16::from(self.nr3) | (u16::from(self.nr4 & 0x07) << 8)
    }

    pub fn period(&self) -> u32 {
        (2048 - u32::from(self.frequency())) * 2
    }

    pub fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        if self.length_counter == 0 {
            self.length_counter = 256;
        }
        self.timer = self.period();
        self.position = 0;
    }

    pub fn clock_timer(&mut self, cycles: u32) {
        if self.timer <= cycles {
            let p = self.period();
            let rem = cycles - self.timer;
            self.position = (self.position + 1 + (rem / p) as usize) % 32;
            self.timer = p - (rem % p);
        } else {
            self.timer -= cycles;
        }
    }

    pub fn clock_length(&mut self) {
        if (self.nr4 & 0x40) != 0 && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn sample(&self) -> u8 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }

        let byte = self.wave_ram[self.position / 2];
        let sample = if self.position.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0F
        };

        match (self.nr2 >> 5) & 0x03 {
            0 => 0,
            1 => sample,
            2 => sample >> 1,
            3 => sample >> 2,
            _ => 0,
        }
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_bool(self.enabled);
        w.write_bool(self.dac_enabled);
        w.write_u8(self.nr0);
        w.write_u8(self.nr1);
        w.write_u8(self.nr2);
        w.write_u8(self.nr3);
        w.write_u8(self.nr4);
        w.write_u32(self.timer);
        w.write_u32(self.position as u32);
        w.write_u16(self.length_counter);
        w.write_raw(&self.wave_ram);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.enabled = r.read_bool()?;
        self.dac_enabled = r.read_bool()?;
        self.nr0 = r.read_u8()?;
        self.nr1 = r.read_u8()?;
        self.nr2 = r.read_u8()?;
        self.nr3 = r.read_u8()?;
        self.nr4 = r.read_u8()?;
        self.timer = r.read_u32()?;
        self.position = r.read_u32()? as usize;
        self.length_counter = r.read_u16()?;
        self.wave_ram.copy_from_slice(r.read_raw(16)?);
        Ok(())
    }
}

/// Noise channel (Channel 4).
#[derive(Debug, Clone)]
pub struct NoiseChannel {
    pub enabled: bool,
    pub dac_enabled: bool,
    pub nr1: u8, // Length load (0-5)
    pub nr2: u8, // Starting volume (4-7), Envelope dir (3), Period (0-2)
    pub nr3: u8, // Clock shift (4-7), LFSR width (3), Divisor code (0-2)
    pub nr4: u8, // Trigger (7), Length enable (6)

    pub timer: u32,
    pub lfsr: u16,
    pub length_counter: u8,
    pub volume: u8,
    pub envelope_timer: u8,
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_enabled: false,
            nr1: 0,
            nr2: 0,
            nr3: 0,
            nr4: 0,
            timer: 0,
            lfsr: 0x7FFF,
            length_counter: 0,
            volume: 0,
            envelope_timer: 0,
        }
    }
}

impl NoiseChannel {
    pub fn period(&self) -> u32 {
        let divisor_code = (self.nr3 & 0x07) as usize;
        let shift = (self.nr3 >> 4) as u32;
        let div = NOISE_DIVISORS[divisor_code];
        div << shift
    }

    pub fn trigger(&mut self) {
        self.enabled = self.dac_enabled;
        if self.length_counter == 0 {
            self.length_counter = 64;
        }
        self.timer = self.period();
        self.lfsr = 0x7FFF;
        self.volume = self.nr2 >> 4;
        self.envelope_timer = self.nr2 & 0x07;
    }

    pub fn clock_timer(&mut self, cycles: u32) {
        let p = self.period();
        if p == 0 {
            return;
        }

        if self.timer <= cycles {
            let rem = cycles - self.timer;
            let steps = 1 + rem / p;
            self.timer = p - (rem % p);

            for _ in 0..steps {
                let xor = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
                self.lfsr = (self.lfsr >> 1) | (xor << 14);
                if (self.nr3 & 0x08) != 0 {
                    // 7-bit mode
                    self.lfsr = (self.lfsr & !(1 << 6)) | (xor << 6);
                }
            }
        } else {
            self.timer -= cycles;
        }
    }

    pub fn clock_length(&mut self) {
        if (self.nr4 & 0x40) != 0 && self.length_counter > 0 {
            self.length_counter -= 1;
            if self.length_counter == 0 {
                self.enabled = false;
            }
        }
    }

    pub fn clock_envelope(&mut self) {
        let period = self.nr2 & 0x07;
        if period == 0 {
            return;
        }

        if self.envelope_timer > 0 {
            self.envelope_timer -= 1;
        }

        if self.envelope_timer == 0 {
            self.envelope_timer = period;
            let direction_up = (self.nr2 & 0x08) != 0;
            if direction_up && self.volume < 15 {
                self.volume += 1;
            } else if !direction_up && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }

    pub fn sample(&self) -> u8 {
        if !self.enabled || !self.dac_enabled {
            return 0;
        }

        if (self.lfsr & 1) == 0 {
            self.volume
        } else {
            0
        }
    }

    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        w.write_bool(self.enabled);
        w.write_bool(self.dac_enabled);
        w.write_u8(self.nr1);
        w.write_u8(self.nr2);
        w.write_u8(self.nr3);
        w.write_u8(self.nr4);
        w.write_u32(self.timer);
        w.write_u16(self.lfsr);
        w.write_u8(self.length_counter);
        w.write_u8(self.volume);
        w.write_u8(self.envelope_timer);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.enabled = r.read_bool()?;
        self.dac_enabled = r.read_bool()?;
        self.nr1 = r.read_u8()?;
        self.nr2 = r.read_u8()?;
        self.nr3 = r.read_u8()?;
        self.nr4 = r.read_u8()?;
        self.timer = r.read_u32()?;
        self.lfsr = r.read_u16()?;
        self.length_counter = r.read_u8()?;
        self.volume = r.read_u8()?;
        self.envelope_timer = r.read_u8()?;
        Ok(())
    }
}

/// The complete Audio Processing Unit.
#[derive(Debug, Clone)]
pub struct Apu {
    pub ch1: PulseChannel,
    pub ch2: PulseChannel,
    pub ch3: WaveChannel,
    pub ch4: NoiseChannel,

    pub nr50: u8, // Master volume left/right
    pub nr51: u8, // Sound panning
    pub nr52: u8, // Master sound enable / channel status

    pub frame_sequencer_timer: u32,
    pub frame_sequencer_step: u8,

    pub sample_accumulator: u32,
    pub samples: Vec<f32>,
}

impl Default for Apu {
    fn default() -> Self {
        Self {
            ch1: PulseChannel::new(true),
            ch2: PulseChannel::new(false),
            ch3: WaveChannel::default(),
            ch4: NoiseChannel::default(),
            nr50: 0x77,
            nr51: 0xF3,
            nr52: 0xF1,
            frame_sequencer_timer: CLOCK_SPEED / 512,
            frame_sequencer_step: 0,
            sample_accumulator: 0,
            samples: Vec::with_capacity(AUDIO_SAMPLE_RATE as usize / 60 * AUDIO_CHANNELS * 2),
        }
    }
}

impl Apu {
    pub(crate) fn save_state(&self, w: &mut StateWriter) {
        self.ch1.save_state(w);
        self.ch2.save_state(w);
        self.ch3.save_state(w);
        self.ch4.save_state(w);
        w.write_u8(self.nr50);
        w.write_u8(self.nr51);
        w.write_u8(self.nr52);
        w.write_u32(self.frame_sequencer_timer);
        w.write_u8(self.frame_sequencer_step);
        w.write_u32(self.sample_accumulator);
    }

    pub(crate) fn load_state(&mut self, r: &mut StateReader) -> Result<(), SaveStateError> {
        self.ch1.load_state(r)?;
        self.ch2.load_state(r)?;
        self.ch3.load_state(r)?;
        self.ch4.load_state(r)?;
        self.nr50 = r.read_u8()?;
        self.nr51 = r.read_u8()?;
        self.nr52 = r.read_u8()?;
        self.frame_sequencer_timer = r.read_u32()?;
        self.frame_sequencer_step = r.read_u8()?;
        self.sample_accumulator = r.read_u32()?;
        self.samples.clear();
        Ok(())
    }

    /// Interleaved stereo `f32` samples produced during the most recent frame.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Clears the sample buffer after consumption by the frontend.
    pub fn clear_samples(&mut self) {
        self.samples.clear();
    }

    /// The sample rate used by the core (48,000 Hz).
    pub const fn sample_rate() -> u32 {
        AUDIO_SAMPLE_RATE
    }

    /// The number of interleaved channels (2: stereo).
    pub const fn channels() -> usize {
        AUDIO_CHANNELS
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => self.ch1.nr0 | 0x80,
            0xFF11 => self.ch1.nr1 | 0x3F,
            0xFF12 => self.ch1.nr2,
            0xFF13 => 0xFF,
            0xFF14 => self.ch1.nr4 | 0xBF,

            0xFF15 => 0xFF,
            0xFF16 => self.ch2.nr1 | 0x3F,
            0xFF17 => self.ch2.nr2,
            0xFF18 => 0xFF,
            0xFF19 => self.ch2.nr4 | 0xBF,

            0xFF1A => self.ch3.nr0 | 0x7F,
            0xFF1B => 0xFF,
            0xFF1C => self.ch3.nr2 | 0x9F,
            0xFF1D => 0xFF,
            0xFF1E => self.ch3.nr4 | 0xBF,

            0xFF1F => 0xFF,
            0xFF20 => self.ch4.nr1 | 0xFF,
            0xFF21 => self.ch4.nr2,
            0xFF22 => self.ch4.nr3,
            0xFF23 => self.ch4.nr4 | 0xBF,

            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            0xFF26 => {
                let status = (if self.ch1.enabled { 1 } else { 0 })
                    | (if self.ch2.enabled { 2 } else { 0 })
                    | (if self.ch3.enabled { 4 } else { 0 })
                    | (if self.ch4.enabled { 8 } else { 0 });
                (self.nr52 & 0x80) | 0x70 | status
            }

            0xFF27..=0xFF2F => 0xFF,
            0xFF30..=0xFF3F => self.ch3.wave_ram[(addr - 0xFF30) as usize],
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        if (self.nr52 & 0x80) == 0 && addr != 0xFF26 && !(0xFF30..=0xFF3F).contains(&addr) {
            return;
        }

        match addr {
            // Channel 1
            0xFF10 => self.ch1.nr0 = value,
            0xFF11 => {
                self.ch1.nr1 = value;
                self.ch1.length_counter = 64 - (value & 0x3F);
            }
            0xFF12 => {
                self.ch1.nr2 = value;
                self.ch1.dac_enabled = (value & 0xF8) != 0;
                if !self.ch1.dac_enabled {
                    self.ch1.enabled = false;
                }
            }
            0xFF13 => self.ch1.nr3 = value,
            0xFF14 => {
                self.ch1.nr4 = value;
                if (value & 0x80) != 0 {
                    self.ch1.trigger();
                }
            }

            // Channel 2
            0xFF16 => {
                self.ch2.nr1 = value;
                self.ch2.length_counter = 64 - (value & 0x3F);
            }
            0xFF17 => {
                self.ch2.nr2 = value;
                self.ch2.dac_enabled = (value & 0xF8) != 0;
                if !self.ch2.dac_enabled {
                    self.ch2.enabled = false;
                }
            }
            0xFF18 => self.ch2.nr3 = value,
            0xFF19 => {
                self.ch2.nr4 = value;
                if (value & 0x80) != 0 {
                    self.ch2.trigger();
                }
            }

            // Channel 3
            0xFF1A => {
                self.ch3.nr0 = value;
                self.ch3.dac_enabled = (value & 0x80) != 0;
                if !self.ch3.dac_enabled {
                    self.ch3.enabled = false;
                }
            }
            0xFF1B => {
                self.ch3.nr1 = value;
                self.ch3.length_counter = 256 - u16::from(value);
            }
            0xFF1C => self.ch3.nr2 = value,
            0xFF1D => self.ch3.nr3 = value,
            0xFF1E => {
                self.ch3.nr4 = value;
                if (value & 0x80) != 0 {
                    self.ch3.trigger();
                }
            }

            // Channel 4
            0xFF20 => {
                self.ch4.nr1 = value;
                self.ch4.length_counter = 64 - (value & 0x3F);
            }
            0xFF21 => {
                self.ch4.nr2 = value;
                self.ch4.dac_enabled = (value & 0xF8) != 0;
                if !self.ch4.dac_enabled {
                    self.ch4.enabled = false;
                }
            }
            0xFF22 => self.ch4.nr3 = value,
            0xFF23 => {
                self.ch4.nr4 = value;
                if (value & 0x80) != 0 {
                    self.ch4.trigger();
                }
            }

            // Master controls
            0xFF24 => self.nr50 = value,
            0xFF25 => self.nr51 = value,
            0xFF26 => {
                let was_enabled = (self.nr52 & 0x80) != 0;
                let now_enabled = (value & 0x80) != 0;
                self.nr52 = value & 0x80;

                if was_enabled && !now_enabled {
                    // Turn APU off: clear all registers (0xFF10..=0xFF25)
                    self.ch1 = PulseChannel::new(true);
                    self.ch2 = PulseChannel::new(false);
                    let wave_ram = self.ch3.wave_ram;
                    self.ch3 = WaveChannel::default();
                    self.ch3.wave_ram = wave_ram;
                    self.ch4 = NoiseChannel::default();
                    self.nr50 = 0;
                    self.nr51 = 0;
                } else if !was_enabled && now_enabled {
                    self.frame_sequencer_step = 0;
                    self.frame_sequencer_timer = CLOCK_SPEED / 512;
                }
            }

            0xFF30..=0xFF3F => {
                self.ch3.wave_ram[(addr - 0xFF30) as usize] = value;
            }
            _ => {}
        }
    }

    /// Advances the APU by `cycles` base T-cycles (4.194304 MHz clock).
    pub fn tick(&mut self, cycles: u32) {
        if (self.nr52 & 0x80) == 0 {
            return;
        }

        // Advance individual channel timers
        self.ch1.clock_timer(cycles);
        self.ch2.clock_timer(cycles);
        self.ch3.clock_timer(cycles);
        self.ch4.clock_timer(cycles);

        // Advance 512 Hz Frame Sequencer
        let mut remaining = cycles;
        while remaining > 0 {
            let step_cycles = remaining.min(self.frame_sequencer_timer);
            self.frame_sequencer_timer -= step_cycles;
            remaining -= step_cycles;

            if self.frame_sequencer_timer == 0 {
                self.frame_sequencer_timer = CLOCK_SPEED / 512;
                self.clock_frame_sequencer();
            }
        }

        // Sample generation at 48,000 Hz
        self.sample_accumulator += cycles * AUDIO_SAMPLE_RATE;
        while self.sample_accumulator >= CLOCK_SPEED {
            self.sample_accumulator -= CLOCK_SPEED;
            self.generate_sample();
        }
    }

    fn clock_frame_sequencer(&mut self) {
        match self.frame_sequencer_step {
            0 => self.clock_length(),
            1 => {}
            2 => {
                self.clock_length();
                self.clock_sweep();
            }
            3 => {}
            4 => self.clock_length(),
            5 => {}
            6 => {
                self.clock_length();
                self.clock_sweep();
            }
            7 => self.clock_envelope(),
            _ => {}
        }
        self.frame_sequencer_step = (self.frame_sequencer_step + 1) % 8;
    }

    fn clock_length(&mut self) {
        self.ch1.clock_length();
        self.ch2.clock_length();
        self.ch3.clock_length();
        self.ch4.clock_length();
    }

    fn clock_sweep(&mut self) {
        self.ch1.clock_sweep();
    }

    fn clock_envelope(&mut self) {
        self.ch1.clock_envelope();
        self.ch2.clock_envelope();
        self.ch4.clock_envelope();
    }

    fn generate_sample(&mut self) {
        // Raw digital samples in 0..15
        let s1 = self.ch1.sample();
        let s2 = self.ch2.sample();
        let s3 = self.ch3.sample();
        let s4 = self.ch4.sample();

        // Convert DAC 0..15 to normalized [-1.0, 1.0]
        let to_dac = |s: u8| -> f32 { (f32::from(s) / 7.5) - 1.0 };

        let v1 = to_dac(s1);
        let v2 = to_dac(s2);
        let v3 = to_dac(s3);
        let v4 = to_dac(s4);

        let mut left = 0.0f32;
        let mut right = 0.0f32;

        let panning = self.nr51;
        // Right channel (bits 0-3)
        if (panning & 0x01) != 0 {
            right += v1;
        }
        if (panning & 0x02) != 0 {
            right += v2;
        }
        if (panning & 0x04) != 0 {
            right += v3;
        }
        if (panning & 0x08) != 0 {
            right += v4;
        }

        // Left channel (bits 4-7)
        if (panning & 0x10) != 0 {
            left += v1;
        }
        if (panning & 0x20) != 0 {
            left += v2;
        }
        if (panning & 0x40) != 0 {
            left += v3;
        }
        if (panning & 0x80) != 0 {
            left += v4;
        }

        let left_vol = f32::from((self.nr50 >> 4) & 0x07) + 1.0;
        let right_vol = f32::from(self.nr50 & 0x07) + 1.0;

        // Scale by master volume and normalize
        let master_gain = 0.25; // 4 channels mixed
        let out_left = (left * master_gain * (left_vol / 8.0)).clamp(-1.0, 1.0);
        let out_right = (right * master_gain * (right_vol / 8.0)).clamp(-1.0, 1.0);

        self.samples.push(out_left);
        self.samples.push(out_right);
    }
}
