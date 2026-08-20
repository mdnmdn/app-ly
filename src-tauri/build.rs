fn main() {
    link_swift_runtime();
    tauri_build::build()
}

/// The Swift runtime is linked in by the on-device AI backend's Swift bridge.
/// Most of it is referenced by absolute path (`/usr/lib/swift/...`) and resolves
/// on its own, but `libswift_Concurrency.dylib` is back-deployable and is
/// referenced as `@rpath/libswift_Concurrency.dylib`. A Rust link emits no
/// `LC_RPATH` entries, so `@rpath` expands to nothing and the app aborts at
/// launch with "Library not loaded" — both when run directly and when bundled.
/// Xcode adds this search path automatically; here it has to be explicit.
///
/// macOS keeps the library in the dyld shared cache rather than on disk, so
/// `/usr/lib/swift` looks empty to `ls` while still being the correct path.
fn link_swift_runtime() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let ai_enabled = std::env::var_os("CARGO_FEATURE_AI_APPLE").is_some();
    if target_os == "macos" && ai_enabled {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
