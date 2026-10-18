fn main() {
    slint_build::compile("ui/app.slint").expect("compile Slint UI");
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() == Some("linux") {
        link_linux_capture_libs();
    }
}

fn link_linux_capture_libs() {
    // libwebrtc's Linux screen capturer references GLib, PipeWire, and GBM,
    // and those flags are not always carried out of the webrtc-sys archive.
    for package in [
        "gio-2.0",
        "gobject-2.0",
        "glib-2.0",
        "libpipewire-0.3",
        "gbm",
        "libdrm",
    ] {
        let output = match std::process::Command::new("pkg-config")
            .args(["--libs", package])
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                println!("cargo:warning=pkg-config failed for {package}: {err}");
                continue;
            }
        };
        if !output.status.success() {
            println!("cargo:warning=pkg-config did not find {package}");
            continue;
        }
        for flag in String::from_utf8_lossy(&output.stdout).split_whitespace() {
            if let Some(name) = flag.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={name}");
            } else if let Some(path) = flag.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={path}");
            }
        }
    }
}
