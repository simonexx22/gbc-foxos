//! cpal audio output.
//!
//! The emulator thread pushes interleaved stereo `f32` samples at 48,000 Hz;
//! this module resamples to the device's native sample rate, maps to the
//! device's channel layout, buffers in a bounded queue, and converts to the
//! device's native sample format in the real-time audio callback.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use gbc_core::AUDIO_SAMPLE_RATE;

/// Resamples 48 kHz stereo input into device-rate, device-channel audio.
struct Resampler {
    in_rate: f64,
    out_rate: f64,
    channels: usize,
    pos: f64,
    last_frame: [f32; 2],
}

impl Resampler {
    fn new(in_rate: u32, out_rate: u32, channels: usize) -> Self {
        Self {
            in_rate: in_rate as f64,
            out_rate: out_rate as f64,
            channels: channels.max(1),
            pos: 0.0,
            last_frame: [0.0, 0.0],
        }
    }

    fn resample_and_map(&mut self, input: &[f32], output: &mut VecDeque<f32>) {
        if input.is_empty() {
            return;
        }

        let input_frames = input.len() / 2;
        if input_frames == 0 {
            return;
        }

        // Fast path: exact 48 kHz stereo match.
        if (self.in_rate - self.out_rate).abs() < 1.0 && self.channels == 2 {
            self.pos = 0.0;
            self.last_frame = [input[input.len() - 2], input[input.len() - 1]];
            output.extend(input.iter().copied());
            return;
        }

        let step = self.in_rate / self.out_rate;

        while self.pos < input_frames as f64 {
            let idx = self.pos.floor() as usize;
            let frac = (self.pos - idx as f64) as f32;

            let prev = if idx == 0 {
                self.last_frame
            } else {
                [input[(idx - 1) * 2], input[(idx - 1) * 2 + 1]]
            };

            let curr = [input[idx * 2], input[idx * 2 + 1]];

            let l = prev[0] + (curr[0] - prev[0]) * frac;
            let r = prev[1] + (curr[1] - prev[1]) * frac;

            match self.channels {
                1 => output.push_back((l + r) * 0.5),
                2 => {
                    output.push_back(l);
                    output.push_back(r);
                }
                n => {
                    output.push_back(l);
                    output.push_back(r);
                    for _ in 2..n {
                        output.push_back(0.0);
                    }
                }
            }

            self.pos += step;
        }

        self.pos -= input_frames as f64;
        self.last_frame = [input[input.len() - 2], input[input.len() - 1]];
    }
}

struct AudioState {
    resampler: Resampler,
    queue: VecDeque<f32>,
    max_queue_len: usize,
}

/// An open audio output stream and its sample queue.
pub struct AudioOutput {
    stream: Stream,
    state: Arc<Mutex<AudioState>>,
    sample_rate: u32,
    channels: usize,
}

impl AudioOutput {
    /// Opens the default output device and starts playback.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device is available")?;
        let supported = device.default_output_config()?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        let sample_rate = config.sample_rate;
        let channels = (config.channels as usize).max(1);

        // Maximum buffer: ~100 ms of audio to prevent unbounded latency drift.
        let max_queue_len = (sample_rate as usize * channels) / 10;

        let state = Arc::new(Mutex::new(AudioState {
            resampler: Resampler::new(AUDIO_SAMPLE_RATE, sample_rate, channels),
            queue: VecDeque::with_capacity(max_queue_len * 2),
            max_queue_len,
        }));

        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, Arc::clone(&state))?,
            SampleFormat::I16 => build_stream::<i16>(&device, &config, Arc::clone(&state))?,
            SampleFormat::U16 => build_stream::<u16>(&device, &config, Arc::clone(&state))?,
            SampleFormat::I8 => build_stream::<i8>(&device, &config, Arc::clone(&state))?,
            SampleFormat::U8 => build_stream::<u8>(&device, &config, Arc::clone(&state))?,
            SampleFormat::I32 => build_stream::<i32>(&device, &config, Arc::clone(&state))?,
            SampleFormat::U32 => build_stream::<u32>(&device, &config, Arc::clone(&state))?,
            other => return Err(format!("unsupported output sample format: {other}").into()),
        };

        stream.play()?;

        Ok(Self {
            stream,
            state,
            sample_rate,
            channels,
        })
    }

    /// Appends interleaved 48 kHz stereo samples to the playback queue.
    pub fn push_samples(&self, samples: &[f32]) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let state = &mut *state;
        state.resampler.resample_and_map(samples, &mut state.queue);

        // Bounded queue: drop oldest samples if falling behind.
        if state.queue.len() > state.max_queue_len {
            let excess = state.queue.len() - state.max_queue_len;
            state.queue.drain(..excess);
        }
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

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    state: Arc<Mutex<AudioState>>,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let error_callback = |error| eprintln!("audio stream error: {error}");

    device.build_output_stream(
        *config,
        move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
            let mut state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            for sample in data {
                let s = state.queue.pop_front().unwrap_or(0.0);
                *sample = T::from_sample(s);
            }
        },
        error_callback,
        None,
    )
}
