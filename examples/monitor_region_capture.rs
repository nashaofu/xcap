use fs_extra::dir;
use std::time::Instant;
use xcap::Monitor;

fn normalized(filename: String) -> String {
    filename.replace(['|', '\\', ':', '/'], "")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let monitors = Monitor::all()?;
    dir::create_all("target/monitors", true).unwrap();

    // Define the region dimensions to capture from each monitor
    let region_width = 400u32;
    let region_height = 300u32;

    // Track total time for all captures
    let total_start = Instant::now();

    println!("Capturing regions from {} monitors...", monitors.len());

    // Iterate through all available monitors
    for monitor in monitors {
        // Calculate center of the monitor for region capture
        let x = 0;
        let y = 0;

        // Capture the region
        let start = Instant::now();
        let image = monitor.capture_region(x, y, region_width, region_height)?;

        // Get monitor name for the filename
        let monitor_name = monitor
            .name()
            .unwrap_or_else(|_| format!("unknown-{}", monitor.id().unwrap_or(0)));
        let is_primary = monitor.is_primary().unwrap_or(false);
        let primary_indicator = if is_primary { "-primary" } else { "" };

        println!(
            "Monitor '{}'{}: Time to capture region of size {}x{}: {:?}",
            monitor_name,
            primary_indicator,
            image.width(),
            image.height(),
            start.elapsed()
        );

        // Save the image
        let filename = format!(
            "target/monitors/monitor-{}{}-region.png",
            normalized(monitor_name),
            primary_indicator
        );

        image.save(&filename).unwrap();
        println!("Saved to {filename}");
    }

    println!(
        "Total time to capture all regions: {:?}",
        total_start.elapsed()
    );

    Ok(())
}
