fn main() {
    #[cfg(target_os = "macos")]
    #[cfg(feature = "mac-window")]
    {
        screenshots::window::Window::default()
            .window_vec
            .iter()
            .filter(|w| w.title.is_some())
            .filter(|w| {
                if let Some(app) = &w.owning_application {
                    if let Some(name) = &app.application_name {
                        name.clone() == *"Arc"
                    } else {
                        false
                    }
                } else {
                    false
                }
            })
            .filter(|w| w.window_layer == 0)
            .for_each(|w| {
                let img = screenshots::window::capture_window(w).unwrap();
                let application_name = &(w)
                    .owning_application
                    .clone()
                    .unwrap()
                    .application_name
                    .unwrap();
                let _ = img.save(format!("target/window-{}.png", application_name));
            });
    }
}
