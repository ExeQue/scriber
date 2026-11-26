//! The library file: `<config>/scriber/library.json` (or `$SCRIBER_LIBRARY`). It holds only what the user
//! added or changed; unchanged built-ins always come fresh from the app, so fixes to them reach the user.

use super::{Library, Machine, Material, builtin_machines, builtin_materials};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bump when the file changes in a way older versions cannot read.
pub const VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LibraryFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub machines: Vec<Machine>,
    #[serde(default)]
    pub materials: Vec<Material>,
    #[serde(default)]
    pub removed_machines: Vec<String>,
    #[serde(default)]
    pub removed_materials: Vec<String>,
}

pub fn path() -> PathBuf {
    match std::env::var_os("SCRIBER_LIBRARY") {
        Some(p) => p.into(),
        None => dirs::config_dir().unwrap_or_default().join("scriber").join("library.json"),
    }
}

/// Built-ins (minus the deleted ones) with the user's machines and materials on top; the user's win.
pub fn merged(file: LibraryFile) -> Library {
    fn layer<T>(builtin: Vec<T>, user: Vec<T>, removed: &[String], id: fn(&T) -> &String) -> Vec<T> {
        let mut out: Vec<T> = builtin.into_iter().filter(|b| !removed.contains(id(b))).collect();
        for u in user {
            super::put(&mut out, u, id);
        }
        out
    }
    Library {
        machines: layer(builtin_machines(), file.machines, &file.removed_machines, |m| &m.id),
        materials: layer(builtin_materials(), file.materials, &file.removed_materials, |m| &m.id),
        removed_machines: file.removed_machines,
        removed_materials: file.removed_materials,
    }
}

/// What goes into the file: everything that is not an unchanged built-in.
fn user_part(lib: &Library) -> LibraryFile {
    let (machines, materials) = (builtin_machines(), builtin_materials());
    LibraryFile {
        version: VERSION,
        machines: lib.machines.iter().filter(|m| !machines.contains(m)).cloned().collect(),
        materials: lib.materials.iter().filter(|m| !materials.contains(m)).cloned().collect(),
        removed_machines: lib.removed_machines.clone(),
        removed_materials: lib.removed_materials.clone(),
    }
}

/// Load the library; a missing file means just the built-ins.
pub fn load(path: &Path) -> anyhow::Result<Library> {
    let file: LibraryFile = match std::fs::read_to_string(path) {
        Ok(json) => serde_json::from_str(&json)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LibraryFile::default(),
        Err(e) => return Err(e.into()),
    };
    anyhow::ensure!(file.version <= VERSION, "the library was saved by a newer Scriber (version {}); please update", file.version);
    Ok(merged(file))
}

/// Like `load`, but a broken file is copied aside (`library.json.broken`) instead of being overwritten later.
pub fn load_or_backup(path: &Path) -> Library {
    load(path).unwrap_or_else(|e| {
        eprintln!("scriber: could not read {}: {e}; using the built-in machines and materials", path.display());
        let _ = std::fs::copy(path, path.with_extension("json.broken"));
        merged(LibraryFile::default())
    })
}

/// Write-then-rename, so a crash never leaves a half-written library.
pub fn save(lib: &Library, path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&user_part(lib))?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Laser, LaserKind, Operation, Setting};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scriber-lib-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("library.json")
    }

    fn k40() -> Machine {
        let laser = Laser { id: "co2".into(), name: "CO2 tube".into(), kind: LaserKind::Co2, power_w: 40.0, ops: Operation::ALL.to_vec(), area: None };
        Machine { id: "k40".into(), name: "K40".into(), vendor: String::new(), model: String::new(), width: 300.0, height: 200.0, lasers: vec![laser], notes: String::new(), source: String::new() }
    }

    #[test]
    fn round_trip_and_atomic_save() {
        let path = temp("rt");
        let mut lib = merged(LibraryFile::default());
        lib.put_machine(k40());
        let mut oak = Material { id: "oak".into(), name: "Oak 4 mm".into(), thickness: 4.0, notes: "smoky".into(), source: String::new(), settings: vec![] };
        let fill = Setting { machine: "k40".into(), laser: "co2".into(), operation: Operation::Fill, power: 30.0, speed: 200.0, passes: 1, line_interval: Some(0.1), bidirectional: Some(true) };
        oak.put(fill.clone());
        oak.put(Setting { power: 35.0, ..fill });
        assert_eq!(oak.settings.len(), 1, "same machine, laser and operation replaces");
        lib.put_material(oak);
        save(&lib, &path).unwrap();
        assert!(!path.with_extension("json.tmp").exists(), "the temp file is renamed away");
        assert_eq!(load(&path).unwrap(), lib);
        // Only the user's part is stored.
        let file: LibraryFile = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((file.version, file.machines.len(), file.materials.len()), (VERSION, 1, 1));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn user_edits_win_over_builtins() {
        let path = temp("merge");
        let mut lib = load(&path).unwrap();
        assert!(lib.machine("xtool-m2").is_some(), "a missing file gives the built-ins");
        let mut m2 = lib.machine("xtool-m2").unwrap().clone();
        m2.notes = "mine".into();
        lib.put_machine(m2);
        lib.remove_material("black-acrylic-3mm");
        save(&lib, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.machine("xtool-m2").unwrap().notes, "mine");
        assert!(back.material("black-acrylic-3mm").is_none(), "deleted built-ins stay deleted");
        assert!(back.material("basswood-plywood-3mm").is_some());
        assert_eq!(back.machines.len(), builtin_machines().len(), "an edited built-in is not listed twice");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn newer_file_is_refused() {
        let path = temp("newer");
        std::fs::write(&path, r#"{"version": 99}"#).unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
