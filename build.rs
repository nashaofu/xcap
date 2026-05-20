fn main() {
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_env != "ohos" {
        return;
    }

    // ── OHOS NDK library search path ─────────────────────────────────────────
    //
    // The OHOS NDK places platform libraries under:
    //   $OHOS_NDK_HOME/sysroot/usr/lib/<arch>-linux-ohos/
    // where OHOS_NDK_HOME points to the `native/` directory inside the SDK.
    //
    // Set the OHOS_NDK_HOME environment variable to the root of your OHOS NDK
    // installation before running `cargo build`. Typical paths:
    //
    //   Linux/macOS:
    //     export OHOS_NDK_HOME=/path/to/ohos-sdk/linux/native
    //
    //   Windows:
    //     set OHOS_NDK_HOME=C:\ohos-sdk\windows\native
    //
    // The `native/` directory contains `sysroot/`, `llvm/`, `build-tools/`, etc.
    //
    // The NDK is bundled with DevEco Studio and can also be downloaded from:
    //   https://developer.huawei.com/consumer/cn/develop
    //
    // Required system libraries (linked via #[link] in src/ohos/ffi.rs):
    //   libnative_display_manager.so   — OH_NativeDisplayManager APIs
    //   libnative_avscreen_capture.so  — OH_AVScreenCapture APIs
    //   libnative_media_core.so        — OH_AVBuffer helpers
    //   libnative_buffer.so            — OH_NativeBuffer helpers

    let ndk_home = std::env::var("OHOS_NDK_HOME").unwrap_or_else(|_| {
        // Fall back to OHOS_SDK_HOME/<platform>/native if OHOS_NDK_HOME is not set.
        // OHOS_SDK_HOME is sometimes set to the SDK root (e.g. ~/.ohos/sdk/default).
        let platform = if cfg!(target_os = "windows") { "windows" } else { "linux" };
        std::env::var("OHOS_SDK_HOME")
            .map(|s| format!("{s}/{platform}/native"))
            .unwrap_or_default()
    });

    if ndk_home.is_empty() {
        // Emit a warning but don't hard-fail; the user may be using a custom
        // linker configuration or cross-compilation wrapper that already
        // provides the necessary search paths.
        println!(
            "cargo:warning=OHOS_NDK_HOME is not set. \
             Make sure the OHOS NDK library paths are available to the linker \
             (libnative_display_manager, libnative_avscreen_capture, \
             libnative_media_core, libnative_buffer)."
        );
        return;
    }

    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // Map Rust target arch names to the OHOS triple component.
    let ohos_arch = match target_arch.as_str() {
        "aarch64" => "aarch64",
        "arm" => "arm",
        "x86_64" => "x86_64",
        other => {
            println!("cargo:warning=Unrecognised OHOS target arch '{other}'. Skipping NDK link-search.");
            return;
        }
    };

    // OHOS_NDK_HOME already points to the `native/` directory, so the sysroot
    // library path is:  $OHOS_NDK_HOME/sysroot/usr/lib/<arch>-linux-ohos/
    let lib_dir = format!("{ndk_home}/sysroot/usr/lib/{ohos_arch}-linux-ohos");
    println!("cargo:rustc-link-search=native={lib_dir}");

    // Some NDK versions also place stub libraries directly under sysroot/usr/lib/.
    let sysroot_lib = format!("{ndk_home}/sysroot/usr/lib");
    println!("cargo:rustc-link-search=native={sysroot_lib}");

    // Re-run this script whenever the NDK home variable changes.
    println!("cargo:rerun-if-env-changed=OHOS_NDK_HOME");
    println!("cargo:rerun-if-env-changed=OHOS_SDK_HOME");
}
