fn main() {
    if option_env!("CARGO_CFG_UNIX").is_some() {
        println!("cargo:rustc-link-lib=dylib=X11");
        println!("cargo:rustc-link-lib=dylib=Xrandr");
    }
}
