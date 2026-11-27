//! Embeds the built-in machine files (`machines/*.json`) and image mode sets
//! (`machines/image-modes/*.json`) in the binary, so adding a machine is just adding a file.
//! Scriber ships no material presets; the material files under `src/library/test_materials/` are
//! fixtures for the tests only.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let mut out = String::new();
    for (dir, name) in [("machines", "MACHINES"), ("machines/image-modes", "IMAGE_MODES"), ("src/library/test_materials", "MATERIALS")] {
        println!("cargo:rerun-if-changed={dir}");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|r| r.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
            .unwrap_or_default();
        files.sort();
        writeln!(out, "pub const {name}: &[(&str, &str)] = &[").unwrap();
        for p in files {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let abs = std::fs::canonicalize(&p).unwrap();
            writeln!(out, "    ({name:?}, include_str!({abs:?})),").unwrap();
        }
        out.push_str("];\n");
    }
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin.rs"), out).unwrap();
    windows_icon();
}

/// The program icon Windows shows for scriber.exe (Explorer, taskbar, shortcuts): assets/scriber.ico,
/// made from docs/public/logo.svg. The window sets its own icon at run time on every system.
#[cfg(windows)]
fn windows_icon() {
    println!("cargo:rerun-if-changed=assets/scriber.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new().set_icon("assets/scriber.ico").compile().expect("embed the Windows icon");
    }
}

#[cfg(not(windows))]
fn windows_icon() {}
