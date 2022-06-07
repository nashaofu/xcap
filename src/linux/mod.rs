mod wayland;
mod x11;

pub fn wayland_capture_display(screen_capturer: &ScreenCapturer) -> Option<Image> {
  if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
    println!("Think Different!");
    wayland_capture_display()
  } else {
    x11_capture_display()
  }
}
