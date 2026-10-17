//! Build glue for the Phase 0 media spike.
//!
//! The LiveKit Rust SDK builds WebRTC with the xdg-desktop-portal screen capture
//! backend on Linux (that is the Wayland path), but `webrtc-sys`'s own build
//! script does not link the GLib stack the portal code uses. Without the links
//! below, any binary that mentions WebRTC's desktop capturer fails to link with
//! undefined `g_variant_*` / `g_dbus_*` / `g_random_*` symbols — the library
//! compiles, the binary does not.
//!
//! This is a real toolchain finding, recorded in `docs/spikes/media-phase0.md`.
//! It is deliberately limited to the `sdk` feature on Linux, and it warns rather
//! than fails when the development packages are missing, so a machine without
//! them still gets a clear message instead of a wall of linker output.

fn main() {
    if std::env::var_os("CARGO_FEATURE_SDK").is_none() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    for library in ["glib-2.0", "gio-2.0", "gobject-2.0", "gmodule-2.0"] {
        if let Err(error) = pkg_config::probe_library(library) {
            println!(
                "cargo:warning=the LiveKit SDK needs {library} to link on Linux ({error}); \
                 install the GLib development package if the build fails"
            );
        }
    }
}
