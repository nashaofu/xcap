//! Record a sub-rectangle of the primary monitor with a frame-rate cap.
//!
//! Run with `cargo run --example monitor_record_region`. The first frame is
//! saved to `target/examples/monitor_record_region.png`, and each frame's
//! `timestamp` (time since the recorder was created) is printed.
//!
//! `Monitor::video_recorder_with_options` is implemented on macOS and Windows.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!(
        "monitor_record_region requires macOS or Windows: VideoRecorderOptions is only implemented there."
    );
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::RecvTimeoutError,
        },
        thread,
        time::Duration,
    };
    use xcap::{CaptureRegion, Monitor, VideoRecorderOptions, image::RgbaImage};

    const FPS: u32 = 15;
    const SECONDS: u64 = 3;
    const OUTPUT: &str = "target/examples/monitor_record_region.png";

    let monitors = Monitor::all()?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or("no monitors found")?;

    // CaptureRegion is in monitor units: points on macOS, physical pixels on
    // Windows. Frame dimensions, in contrast, are always physical pixels.
    let width = 640u32.min(monitor.width()?);
    let height = 400u32.min(monitor.height()?);
    let region = CaptureRegion {
        x: (monitor.width()? - width) / 2,
        y: (monitor.height()? - height) / 2,
        width,
        height,
    };

    let options = VideoRecorderOptions::default()
        .fps(FPS)
        .region(region)
        .captures_cursor(true)
        .excludes_own_windows(true);

    println!(
        "recording {:?} of '{}' (scale factor {}x) at up to {FPS} fps for {SECONDS}s",
        region,
        monitor.friendly_name()?,
        monitor.scale_factor()?
    );
    println!("frame pixels = region size x scale factor on macOS");

    let (recorder, frames) = monitor.video_recorder_with_options(options)?;

    // The sender stays alive as long as the recorder does, so `recv()` never
    // disconnects after `stop()`. Use a timeout plus a stop flag instead.
    let done = Arc::new(AtomicBool::new(false));
    let worker_done = Arc::clone(&done);
    let worker = thread::spawn(move || {
        let mut count = 0u32;
        let mut first: Option<Duration> = None;
        let mut last: Option<Duration> = None;
        let mut size = (0u32, 0u32);

        while !worker_done.load(Ordering::Acquire) {
            match frames.recv_timeout(Duration::from_millis(200)) {
                Ok(frame) => {
                    count += 1;
                    size = (frame.width, frame.height);
                    first.get_or_insert(frame.timestamp);
                    let delta = frame.timestamp - last.unwrap_or(Duration::ZERO);
                    println!(
                        "frame {count:>3}: {}x{}, timestamp {:?}, delta {:?}",
                        frame.width, frame.height, frame.timestamp, delta
                    );
                    last = Some(frame.timestamp);

                    if count == 1 {
                        fs::create_dir_all("target/examples").expect("create output directory");
                        RgbaImage::from_raw(frame.width, frame.height, frame.raw)
                            .expect("frame buffer matches the reported dimensions")
                            .save(OUTPUT)
                            .expect("save first frame");
                        println!("saved first frame to {OUTPUT}");
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        (count, first, last, size)
    });

    recorder.start()?;
    thread::sleep(Duration::from_secs(SECONDS));
    recorder.stop()?;

    // Let the already queued frames drain before stopping the worker.
    thread::sleep(Duration::from_millis(300));
    done.store(true, Ordering::Release);
    let (count, first, last, size) = worker.join().expect("frame worker panicked");

    let span = match (first, last) {
        (Some(first), Some(last)) => last - first,
        _ => Duration::ZERO,
    };
    let measured_fps = if span.is_zero() {
        0.0
    } else {
        count.saturating_sub(1) as f64 / span.as_secs_f64()
    };
    println!(
        "done: {count} frames, {}x{}, span {span:?}, measured {measured_fps:.1} fps",
        size.0, size.1
    );

    Ok(())
}
