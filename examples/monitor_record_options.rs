//! Exercise every `VideoRecorderOptions` field from the command line.
//!
//! ```sh
//! cargo run --example monitor_record_options -- --fps 10 --width 800 --height 600
//! cargo run --example monitor_record_options -- --full --seconds 5 --cursor off
//! cargo run --example monitor_record_options -- --help
//! ```
//!
//! `captures_cursor` and `excludes_own_windows` are macOS-only: Windows
//! duplication never includes the cursor, and Windows callers exclude their
//! own windows themselves. `Monitor::video_recorder_with_options` is
//! implemented on macOS and Windows.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!(
        "monitor_record_options requires macOS or Windows: VideoRecorderOptions is only implemented there."
    );
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    if let Err(error) = app::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod app {
    use std::{
        env, fs,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::RecvTimeoutError,
        },
        thread,
        time::Duration,
    };
    use xcap::{CaptureRegion, Monitor, VideoRecorderOptions, image::RgbaImage};

    const USAGE: &str = "\
Usage: cargo run --example monitor_record_options -- [OPTIONS]

  --fps <1..60>              Frame-rate cap (default 20)
  --x <px> --y <px>          Region origin in monitor units (default: centered)
  --width <px> --height <px> Region size in monitor units (default 640x400)
  --full                     Record the whole monitor (no region)
  --cursor <on|off>          Draw the cursor, macOS only (default on)
  --own-windows <on|off>     Exclude this process' windows, macOS only (default on)
  --seconds <n>              Recording duration (default 4)
  --out <path>               Where to save the first frame
                             (default target/examples/monitor_record_options.png)
  -h, --help                 Show this help
";

    #[derive(Debug)]
    struct Args {
        fps: u32,
        x: Option<u32>,
        y: Option<u32>,
        width: u32,
        height: u32,
        full: bool,
        cursor: bool,
        own_windows: bool,
        seconds: u64,
        out: String,
    }

    impl Default for Args {
        fn default() -> Self {
            Self {
                fps: 20,
                x: None,
                y: None,
                width: 640,
                height: 400,
                full: false,
                cursor: true,
                own_windows: true,
                seconds: 4,
                out: "target/examples/monitor_record_options.png".into(),
            }
        }
    }

    fn take<I>(iter: &mut I, flag: &str) -> Result<String, String>
    where
        I: Iterator<Item = String>,
    {
        iter.next()
            .ok_or_else(|| format!("missing value for {flag}"))
    }

    fn number<T: std::str::FromStr>(
        iter: &mut impl Iterator<Item = String>,
        flag: &str,
    ) -> Result<T, String> {
        let raw = take(iter, flag)?;
        raw.parse::<T>()
            .map_err(|_| format!("invalid value for {flag}: {raw}"))
    }

    fn boolean(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<bool, String> {
        match take(iter, flag)?.as_str() {
            "on" | "true" | "1" => Ok(true),
            "off" | "false" | "0" => Ok(false),
            other => Err(format!(
                "invalid value for {flag}: {other} (expected on/off)"
            )),
        }
    }

    fn parse_args() -> Result<Args, String> {
        let mut args = Args::default();
        let mut iter = env::args().skip(1);

        while let Some(flag) = iter.next() {
            match flag.as_str() {
                "-h" | "--help" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                "--fps" => args.fps = number(&mut iter, &flag)?,
                "--x" => args.x = Some(number(&mut iter, &flag)?),
                "--y" => args.y = Some(number(&mut iter, &flag)?),
                "--width" => args.width = number(&mut iter, &flag)?,
                "--height" => args.height = number(&mut iter, &flag)?,
                "--full" => args.full = true,
                "--cursor" => args.cursor = boolean(&mut iter, &flag)?,
                "--own-windows" => args.own_windows = boolean(&mut iter, &flag)?,
                "--seconds" => args.seconds = number(&mut iter, &flag)?,
                "--out" => args.out = take(&mut iter, &flag)?,
                other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
            }
        }

        Ok(args)
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args = parse_args()?;
        let monitors = Monitor::all()?;
        let monitor = monitors
            .iter()
            .find(|monitor| monitor.is_primary().unwrap_or(false))
            .or_else(|| monitors.first())
            .ok_or("no monitors found")?;

        let monitor_width = monitor.width()?;
        let monitor_height = monitor.height()?;

        let region = if args.full {
            None
        } else {
            let width = args.width.min(monitor_width).max(1);
            let height = args.height.min(monitor_height).max(1);
            Some(CaptureRegion {
                x: args
                    .x
                    .unwrap_or((monitor_width - width) / 2)
                    .min(monitor_width - width),
                y: args
                    .y
                    .unwrap_or((monitor_height - height) / 2)
                    .min(monitor_height - height),
                width,
                height,
            })
        };

        let mut options = VideoRecorderOptions::default()
            .fps(args.fps)
            .captures_cursor(args.cursor)
            .excludes_own_windows(args.own_windows);
        if let Some(region) = region {
            options = options.region(region);
        }

        println!(
            "monitor '{}' ({}x{} in monitor units, scale factor {}x), fps <= {}, region {:?}, cursor {}, exclude own windows {}, {}s",
            monitor.friendly_name()?,
            monitor_width,
            monitor_height,
            monitor.scale_factor()?,
            options.fps,
            options.region,
            options.captures_cursor,
            options.excludes_own_windows,
            args.seconds
        );

        let (recorder, frames) = monitor.video_recorder_with_options(options)?;

        let done = Arc::new(AtomicBool::new(false));
        let worker_done = Arc::clone(&done);
        let out = args.out.clone();
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
                        last = Some(frame.timestamp);
                        if count <= 3 || count % 30 == 0 {
                            println!(
                                "frame {count:>3}: {}x{}, timestamp {:?}",
                                frame.width, frame.height, frame.timestamp
                            );
                        }

                        if count == 1 {
                            if let Some(parent) = std::path::Path::new(&out).parent() {
                                fs::create_dir_all(parent).expect("create output directory");
                            }
                            RgbaImage::from_raw(frame.width, frame.height, frame.raw)
                                .expect("frame buffer matches the reported dimensions")
                                .save(&out)
                                .expect("save first frame");
                            println!("saved first frame to {out}");
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }

            (count, first, last, size)
        });

        recorder.start()?;
        thread::sleep(Duration::from_secs(args.seconds));
        recorder.stop()?;

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
}
