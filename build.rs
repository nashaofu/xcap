fn main() {
  #[cfg(target_os = "linux")]
  println!("cargo:rustc-link-lib=dylib=X11");
  #[cfg(target_os = "linux")]
  println!("cargo:rustc-link-lib=dylib=Xrandr");
}
