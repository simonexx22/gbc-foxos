//! cpal audio output.
//!
//! The emulator thread pushes interleaved stereo `f32` samples at 48,000 Hz;
//! this module resamples to the device's native sample rate, maps to the
//! device's channel layout, buffers in a lock-free bounded SPSC queue (`ringbuf`),
//! and converts to the device's native sample format in the real-time audio callback.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use gbc_core::AUDIO_SAMPLE_RATE;
use ringbuf::traits::*;
use ringbuf::HeapRb;

/// Audio diagnostics counters and metrics.
pub struct AudioDiagnostics {
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub samples_produced: AtomicU64,
    pub samples_consumed: AtomicU64,
    pub current_fill: AtomicUsize,
    pub enabled: bool,
    last_log: std::sync::Mutex<Instant>,
    last_produced: AtomicU64,
    last_consumed: AtomicU64,
}

impl Default for AudioDiagnostics {
    fn default() -> Self {
        let enabled =
            std::env::var("GBC_AUDIO_DEBUG").is_ok() || std::env::var("AUDIO_DEBUG").is_ok();
        Self {
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            samples_produced: AtomicU64::new(0),
            samples_consumed: AtomicU64::new(0),
            current_fill: AtomicUsize::new(0),
            enabled,
            last_log: std::sync::Mutex::new(Instant::now()),
            last_produced: AtomicU64::new(0),
            last_consumed: AtomicU64::new(0),
        }
    }
}

impl AudioDiagnostics {
    /// Periodically logs audio metrics (once per second) if diagnostics are enabled.
    pub fn maybe_log(&self, sample_rate: u32, channels: usize) {
        if !self.enabled {
            return;
        }

        if let Ok(mut last_log) = self.last_log.try_lock() {
            if last_log.elapsed() >= Duration::from_secs(1) {
                let elapsed_secs = last_log.elapsed().as_secs_f64();
                *last_log = Instant::now();

                let underruns = self.underruns.load(Ordering::Relaxed);
                let overruns = self.overruns.load(Ordering::Relaxed);
                let produced = self.samples_produced.load(Ordering::Relaxed);
                let consumed = self.samples_consumed.load(Ordering::Relaxed);
                let fill = self.current_fill.load(Ordering::Relaxed);

                let prev_prod = self.last_produced.swap(produced, Ordering::Relaxed);
                let prev_cons = self.last_consumed.swap(consumed, Ordering::Relaxed);

                let rate_prod =
                    ((produced.saturating_sub(prev_prod)) as f64 / elapsed_secs).round() as u64;
                let rate_cons =
                    ((consumed.saturating_sub(prev_cons)) as f64 / elapsed_secs).round() as u64;

                let fill_ms = (fill as f64 / (sample_rate as f64 * channels as f64)) * 1000.0;

                eprintln!(
                    "[AUDIO DIAG] Fill: {} samples ({:.1} ms) | Underruns: {} | Overruns: {} | Produced: {}/s | Consumed: {}/s",
                    fill, fill_ms, underruns, overruns, rate_prod, rate_cons
                );
            }
        }
    }
}

/// Resamples 48 kHz stereo input into device-rate, device-channel audio with linear interpolation.
pub struct Resampler {
    in_rate: f64,
    out_rate: f64,
    channels: usize,
    phase: f64,
    prev_frame: [f32; 2],
    has_prev: bool,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Self {
        Self {
            in_rate: in_rate as f64,
            out_rate: out_rate as f64,
            channels: channels.max(1),
            phase: 0.0,
            prev_frame: [0.0, 0.0],
            has_prev: false,
        }
    }

    #[inline]
    fn emit_frame(l: f32, r: f32, channels: usize, out: &mut Vec<f32>) {
        match channels {
            1 => out.push((l + r) * 0.5),
            2 => {
                out.push(l);
                out.push(r);
            }
            n => {
                out.push(l);
                out.push(r);
                for _ in 2..n {
                    out.push(0.0);
                }
            }
        }
    }

    pub fn resample_and_map(&mut self, input: &[f32], volume: f32, out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }

        let num_frames = input.len() / 2;
        if num_frames == 0 {
            return;
        }

        // Fast path: exact 48 kHz stereo match.
        if (self.in_rate - self.out_rate).abs() < 1.0 && self.channels == 2 {
            self.has_prev = true;
            self.prev_frame = [input[input.len() - 2], input[input.len() - 1]];
            self.phase = 0.0;
            if (volume - 1.0).abs() < 0.001 {
                out.extend_from_slice(input);
            } else {
                out.reserve(input.len());
                for &s in input {
                    out.push(s * volume);
                }
            }
            return;
        }

        let step = self.in_rate / self.out_rate;
        let mut in_idx = 0;

        if !self.has_prev {
            self.prev_frame = [input[0], input[1]];
            self.has_prev = true;
            in_idx = 1;
        }

        while in_idx < num_frames {
            let next_frame = [input[in_idx * 2], input[in_idx * 2 + 1]];
            while self.phase < 1.0 {
                let frac = self.phase as f32;
                let l = (self.prev_frame[0] + (next_frame[0] - self.prev_frame[0]) * frac) * volume;
                let r = (self.prev_frame[1] + (next_frame[1] - self.prev_frame[1]) * frac) * volume;
                Self::emit_frame(l, r, self.channels, out);
                self.phase += step;
            }
            self.phase -= 1.0;
            self.prev_frame = next_frame;
            in_idx += 1;
        }
    }
}

/// An open audio output stream and its lock-free ring buffer producer.
pub struct AudioOutput {
    stream: Stream,
    producer: ringbuf::HeapProd<f32>,
    resampler: Resampler,
    sample_rate: u32,
    channels: usize,
    diagnostics: Arc<AudioDiagnostics>,
    resample_buf: Vec<f32>,
}

impl AudioOutput {
    /// Opens the default output device and starts playback with the specified buffer configuration.
    pub fn new(buffer_size: usize) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device is available")?;
        let supported = device.default_output_config()?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        let sample_rate = config.sample_rate;
        let channels = (config.channels as usize).max(1);

        // Capacity for ~250 ms of audio (at least 4x target fill or buffer_size)
        let capacity_samples = (sample_rate as usize * channels / 4)
            .max(buffer_size * channels * 4)
            .max(8192);

        let rb = HeapRb::<f32>::new(capacity_samples);
        let (mut producer, consumer) = rb.split();

        // Target fill around 75 ms (50-100 ms range)
        let target_fill_frames = (sample_rate as f64 * 0.075).round() as usize;
        let target_fill_samples = target_fill_frames * channels;

        // Pre-roll with 75 ms of silence so playback begins without initial underruns
        let silence = vec![0.0f32; target_fill_samples];
        producer.push_slice(&silence);

        let diagnostics = Arc::new(AudioDiagnostics::default());
        diagnostics
            .current_fill
            .store(producer.occupied_len(), Ordering::Relaxed);
        diagnostics
            .samples_produced
            .store(target_fill_samples as u64, Ordering::Relaxed);

        let stream = match sample_format {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            SampleFormat::I16 => {
                build_stream::<i16>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            SampleFormat::U16 => {
                build_stream::<u16>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            SampleFormat::I8 => {
                build_stream::<i8>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            SampleFormat::U8 => {
                build_stream::<u8>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            SampleFormat::I32 => {
                build_stream::<i32>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            SampleFormat::U32 => {
                build_stream::<u32>(&device, &config, consumer, Arc::clone(&diagnostics))?
            }
            other => return Err(format!("unsupported output sample format: {other}").into()),
        };

        stream.play()?;

        Ok(Self {
            stream,
            producer,
            resampler: Resampler::new(AUDIO_SAMPLE_RATE, sample_rate, channels),
            sample_rate,
            channels,
            diagnostics,
            resample_buf: Vec::with_capacity(4096),
        })
    }

    /// Access to audio diagnostics.
    #[allow(dead_code)]
    pub fn diagnostics(&self) -> &Arc<AudioDiagnostics> {
        &self.diagnostics
    }

    /// Number of samples currently held in the ring buffer.
    #[allow(dead_code)]
    pub fn buffer_fill_samples(&self) -> usize {
        self.producer.occupied_len()
    }

    /// Current buffer fill level in milliseconds.
    pub fn buffer_fill_ms(&self) -> f64 {
        let fill = self.producer.occupied_len();
        (fill as f64 / (self.sample_rate as f64 * self.channels as f64)) * 1000.0
    }

    /// Calculates dynamic rate control multiplier for frame pacing.
    ///
    /// Keeps buffer around 50-100 ms full (target: 75 ms).
    /// Returns a factor near 1.0 (clamped between 0.90 and 1.10).
    pub fn dynamic_rate_multiplier(&self) -> f64 {
        let fill_ms = self.buffer_fill_ms();
        let diff_ms = fill_ms - 75.0;

        if fill_ms < 20.0 {
            0.90 // Buffer running critically low: speed up emulation 10%
        } else if fill_ms > 130.0 {
            1.10 // Buffer filling too high: slow down emulation 10%
        } else {
            // Proportional adjustment: ~0.2% per ms of deviation
            (1.0 + diff_ms * 0.002).clamp(0.97, 1.03)
        }
    }

    /// Appends interleaved 48 kHz stereo samples to the lock-free ring buffer.
    pub fn push_samples(&mut self, samples: &[f32], volume: f32, mute: bool) {
        if samples.is_empty() {
            return;
        }

        self.resample_buf.clear();

        if mute || volume <= 0.0 {
            // Push silence corresponding to input frame duration to prevent buffer starvation during mute
            let input_frames = samples.len() / 2;
            let out_frames = ((input_frames as f64 * self.sample_rate as f64)
                / AUDIO_SAMPLE_RATE as f64)
                .round() as usize;
            let out_samples = out_frames * self.channels;
            self.resample_buf.resize(out_samples, 0.0);
        } else {
            self.resampler
                .resample_and_map(samples, volume, &mut self.resample_buf);
        }

        if !self.resample_buf.is_empty() {
            let to_push = &self.resample_buf[..];
            let pushed = self.producer.push_slice(to_push);
            self.diagnostics
                .samples_produced
                .fetch_add(pushed as u64, Ordering::Relaxed);

            if pushed < to_push.len() {
                let overrun = (to_push.len() - pushed) as u64;
                self.diagnostics
                    .overruns
                    .fetch_add(overrun, Ordering::Relaxed);
            }
        }

        let fill = self.producer.occupied_len();
        self.diagnostics.current_fill.store(fill, Ordering::Relaxed);

        self.diagnostics.maybe_log(self.sample_rate, self.channels);
    }

    /// Pauses audio output.
    pub fn pause(&self) -> Result<(), cpal::Error> {
        self.stream.pause()
    }

    /// Resumes audio output.
    pub fn play(&self) -> Result<(), cpal::Error> {
        self.stream.play()
    }

    /// Sample rate of the open output device.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channel count of the open output device.
    pub fn channels(&self) -> usize {
        self.channels
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        let _ = self.stream.pause();
    }
}

/// Builds the cpal output stream.
///
/// Lock-free, allocation-free, and real-time safe: consumes from `ringbuf::HeapCons`.
fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: ringbuf::HeapCons<f32>,
    diagnostics: Arc<AudioDiagnostics>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let error_callback = |error| eprintln!("audio stream error: {error}");

    device.build_output_stream(
        *config,
        move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
            let mut underrun_samples = 0u64;
            let mut consumed = 0u64;

            let mut temp = [0.0f32; 128];
            for chunk in data.chunks_mut(128) {
                let n = consumer.pop_slice(&mut temp[..chunk.len()]);
                for (dst, &src) in chunk[..n].iter_mut().zip(&temp[..n]) {
                    *dst = T::from_sample(src);
                }
                for dst in &mut chunk[n..] {
                    *dst = T::from_sample(0.0);
                }
                consumed += chunk.len() as u64;
                underrun_samples += (chunk.len() - n) as u64;
            }

            diagnostics
                .samples_consumed
                .fetch_add(consumed, Ordering::Relaxed);
            if underrun_samples > 0 {
                diagnostics
                    .underruns
                    .fetch_add(underrun_samples, Ordering::Relaxed);
            }
        },
        error_callback,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_identity() {
        let mut resampler = Resampler::new(48000, 48000, 2);
        let input = [0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6];
        let mut out = Vec::new();
        resampler.resample_and_map(&input, 1.0, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn test_resampler_mono_mixdown() {
        let mut resampler = Resampler::new(48000, 48000, 1);
        let input = [0.2f32, 0.4, 0.6, 0.8];
        let mut out = Vec::new();
        resampler.resample_and_map(&input, 1.0, &mut out);
        assert!(!out.is_empty());
        for &s in &out {
            assert!((0.0..=1.0).contains(&s));
        }
    }

    #[test]
    fn test_resampler_sample_rate_conversion() {
        let mut resampler = Resampler::new(48000, 44100, 2);
        let input = vec![0.5f32; 1600]; // 800 stereo frames
        let mut out = Vec::new();
        resampler.resample_and_map(&input, 1.0, &mut out);
        // 800 * 44100 / 48000 = 735 frames = 1470 samples
        let frames_out = out.len() / 2;
        assert!(
            (frames_out as i32 - 735).abs() <= 5,
            "expected ~735 frames, got {frames_out}"
        );
    }

    #[test]
    fn test_ringbuf_lockfree_transfer() {
        let rb = HeapRb::<f32>::new(1024);
        let (mut prod, mut cons) = rb.split();

        let input = [0.25f32, -0.5, 0.75, -1.0];
        assert_eq!(prod.push_slice(&input), 4);
        assert_eq!(cons.occupied_len(), 4);

        let mut output = [0.0f32; 4];
        assert_eq!(cons.pop_slice(&mut output), 4);
        assert_eq!(output, input);
        assert_eq!(cons.occupied_len(), 0);
    }
}
