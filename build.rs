fn main() {
    // Link X11 library for window title detection on Linux
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=X11");
    }
}