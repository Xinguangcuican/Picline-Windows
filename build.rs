//! Compiles the icon resource into the exe.
//!
//! Rust has no way to say "attach this .rc file" for GNU targets, so the
//! resource is compiled to a COFF object with windres and handed to the linker.
//! If windres is not around — someone building this on Windows with MSVC, say —
//! the build still succeeds and the exe simply has no icon.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=res/picline.rc");
    println!("cargo:rerun-if-changed=res/picline.ico");

    let arch = if target.starts_with("aarch64") {
        "aarch64"
    } else if target.starts_with("i686") {
        "i686"
    } else {
        "x86_64"
    };
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("picline-res.o");

    // PICLINE_MINGW points at a cross toolchain; otherwise the arch-prefixed
    // windres has to be on PATH.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(root) = env::var("PICLINE_MINGW") {
        candidates.push(PathBuf::from(root).join("bin").join(format!("{arch}-w64-mingw32-windres")));
    }
    if let Ok(home) = env::var("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join("toolchains/llvm-mingw-20261020-ucrt-macos-universal/bin")
                .join(format!("{arch}-w64-mingw32-windres")),
        );
    }
    candidates.push(PathBuf::from(format!("{arch}-w64-mingw32-windres")));

    for windres in candidates {
        let status = Command::new(&windres)
            .current_dir(&manifest)
            .args(["res/picline.rc", "-O", "coff", "-o"])
            .arg(&out)
            .status();
        if let Ok(status) = status {
            if status.success() {
                println!("cargo:rustc-link-arg={}", out.display());
                return;
            }
        }
    }
    println!("cargo:warning=windres not found; building without the app icon");
}
