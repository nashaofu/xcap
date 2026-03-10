use xcap::Window;

fn main() {
    let query = std::env::args().nth(1).expect("Usage: screenshot <window-name-substring>");
    let windows = Window::all().unwrap();

    println!("Found {} windows total", windows.len());

    let mut found = false;
    for window in &windows {
        let title = window.title().unwrap_or_default();
        let app = window.app_name().unwrap_or_default();
        if title.contains(&query) || app.contains(&query) {
            found = true;
            println!(
                "MATCH: app={:?} title={:?} pos=({}, {}) size={}x{} minimized={} focused={}",
                app,
                title,
                window.x().unwrap(),
                window.y().unwrap(),
                window.width().unwrap(),
                window.height().unwrap(),
                window.is_minimized().unwrap(),
                window.is_focused().unwrap(),
            );

            let filename = format!("target/{}.png", title.replace(['|', '\\', ':', '/', '"'], ""));
            match window.capture_image() {
                Ok(image) => {
                    image.save(&filename).unwrap();
                    println!("Saved capture to {filename} ({}x{})", image.width(), image.height());
                }
                Err(e) => println!("capture_image() failed: {e}"),
            }
        }
    }

    if !found {
        println!("No window matching {:?} found", query);
    }
}
