//! Embeds the built-in machine and material files (`machines/*.json`, `materials/*.json`) in the binary,
//! so adding a machine is just adding a file.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let mut out = String::new();
    for dir in ["machines", "materials"] {
        println!("cargo:rerun-if-changed={dir}");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map(|r| r.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
            .unwrap_or_default();
        files.sort();
        writeln!(out, "pub const {}: &[(&str, &str)] = &[", dir.to_uppercase()).unwrap();
        for p in files {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let abs = std::fs::canonicalize(&p).unwrap();
            writeln!(out, "    ({name:?}, include_str!({abs:?})),").unwrap();
        }
        out.push_str("];\n");
    }
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin.rs"), out).unwrap();
}
