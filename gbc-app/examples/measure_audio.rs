use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    std::env::set_var("GBC_AUDIO_DEBUG", "1");
    println!("Starting audio measurement...");

    let rom_path = PathBuf::from("gbc-core/tests/roms/tetris.gb");
    let rom = match std::fs::read(&rom_path) {
        Ok(data) => data,
        Err(_) => {
            println!("Reading from ../gbc-core/tests/roms/tetris.gb");
            std::fs::read("../gbc-core/tests/roms/tetris.gb").unwrap()
        }
    };

    let mut gameboy = gbc_core::Gameboy::new(rom);
    let mut audio = match gbc_app::audio::AudioOutput::new(1024) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to open audio device: {e}");
            return;
        }
    };

    println!(
        "Audio device opened: {} Hz, {} channels",
        audio.sample_rate(),
        audio.channels()
    );

    let frame_interval = Duration::from_nanos(16_742_706);
    let start = Instant::now();
    let mut next_frame = Instant::now();

    // Run for 5 seconds
    while start.elapsed() < Duration::from_secs(5) {
        let now = Instant::now();
        if now >= next_frame {
            gameboy.step_frame();
            audio.push_samples(gameboy.audio_samples(), 1.0, false);

            let factor = audio.dynamic_rate_multiplier();
            let interval = frame_interval.mul_f64(factor);

            next_frame += interval;
            if next_frame < now {
                if (now - next_frame) > Duration::from_millis(100) {
                    next_frame = now + interval;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let diag = audio.diagnostics();
    println!("=== AUDIO RESULTS (AFTER) ===");
    println!(
        "Underruns: {}",
        diag.underruns.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Overruns:  {}",
        diag.overruns.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Produced:  {}",
        diag.samples_produced
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Consumed:  {}",
        diag.samples_consumed
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    println!(
        "Fill:      {} samples ({:.1} ms)",
        diag.current_fill.load(std::sync::atomic::Ordering::Relaxed),
        audio.buffer_fill_ms()
    );
}
