//! Library files (`.scrlib`) for sharing machines and materials with other Scriber users.

use super::{Library, Machine, Material};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const FORMAT: &str = "scriber-library";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "scrlib";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub machines: Vec<Machine>,
    #[serde(default)]
    pub materials: Vec<Material>,
}

/// What to do when an imported machine or material has the id of a different one already in the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    Replace,
    /// Import it under a new id.
    KeepBoth,
    Skip,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub skipped: Vec<String>,
    /// Already in the library, the same.
    pub unchanged: Vec<String>,
    /// Machines that settings refer to but neither the file nor the library has; those settings are kept.
    pub missing_machines: Vec<String>,
}

/// The chosen machines and materials (both None: everything), plus the machines the materials' settings use.
pub fn bundle(lib: &Library, machines: Option<&[String]>, materials: Option<&[String]>) -> Result<Bundle, String> {
    let all = machines.is_none() && materials.is_none();
    let mut mats = Vec::new();
    for id in materials.unwrap_or_default() {
        mats.push(lib.material(id).cloned().ok_or(format!("no material with id {id}"))?);
    }
    if all {
        mats = lib.materials.clone();
    }
    let mut ids: Vec<String> = if all { lib.machines.iter().map(|m| m.id.clone()).collect() } else { machines.unwrap_or_default().to_vec() };
    for s in mats.iter().flat_map(|m| &m.settings) {
        if !ids.contains(&s.machine) && lib.machine(&s.machine).is_some() {
            ids.push(s.machine.clone());
        }
    }
    let machines = ids.iter().map(|id| lib.machine(id).cloned().ok_or(format!("no machine with id {id}"))).collect::<Result<_, _>>()?;
    Ok(Bundle { format: FORMAT.into(), version: VERSION, machines, materials: mats })
}

pub fn write(b: &Bundle, path: &Path) -> Result<(), String> {
    std::fs::write(path, serde_json::to_string_pretty(b).unwrap()).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read(path: &Path) -> Result<Bundle, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let b: Bundle = serde_json::from_str(&text).map_err(|e| format!("{} is not a Scriber library file: {e}", path.display()))?;
    if b.format != FORMAT {
        return Err(format!("{} is not a Scriber library file", path.display()));
    }
    if b.version > VERSION {
        return Err(format!("{} was made by a newer Scriber (version {}); please update", path.display(), b.version));
    }
    Ok(b)
}

/// How many machines and materials in the bundle have the id of a different one in the library.
pub fn conflicts(lib: &Library, b: &Bundle) -> usize {
    b.machines.iter().filter(|m| lib.machine(&m.id).is_some_and(|o| o != *m)).count() + b.materials.iter().filter(|m| lib.material(&m.id).is_some_and(|o| o != *m)).count()
}

/// Merge the bundle into the library. Machines go first, so a renamed machine's materials follow it.
pub fn import(lib: &mut Library, b: Bundle, on: OnConflict) -> Report {
    let mut r = Report::default();
    let mut renamed: HashMap<String, String> = HashMap::new();
    for mut m in b.machines {
        match lib.machine(&m.id) {
            None => {
                r.added.push(m.id.clone());
                lib.put_machine(m);
            }
            Some(o) if *o == m => r.unchanged.push(m.id.clone()),
            Some(_) => match on {
                OnConflict::Replace => {
                    r.replaced.push(m.id.clone());
                    lib.put_machine(m);
                }
                OnConflict::Skip => r.skipped.push(m.id.clone()),
                OnConflict::KeepBoth => {
                    let id = lib.new_id(&m.name);
                    renamed.insert(m.id.clone(), id.clone());
                    m.id = id;
                    r.added.push(m.id.clone());
                    lib.put_machine(m);
                }
            },
        }
    }
    for mut m in b.materials {
        for s in &mut m.settings {
            if let Some(id) = renamed.get(&s.machine) {
                s.machine = id.clone();
            }
            if lib.machine(&s.machine).is_none() && !r.missing_machines.contains(&s.machine) {
                r.missing_machines.push(s.machine.clone());
            }
        }
        match lib.material(&m.id) {
            None => {
                r.added.push(m.id.clone());
                lib.put_material(m);
            }
            Some(o) if *o == m => r.unchanged.push(m.id.clone()),
            Some(_) => match on {
                OnConflict::Replace => {
                    r.replaced.push(m.id.clone());
                    lib.put_material(m);
                }
                OnConflict::Skip => r.skipped.push(m.id.clone()),
                OnConflict::KeepBoth => {
                    m.id = lib.new_id(&m.name);
                    r.added.push(m.id.clone());
                    lib.put_material(m);
                }
            },
        }
    }
    r
}

impl Report {
    /// One line for people, e.g. "2 added, 1 replaced".
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = [("added", &self.added), ("replaced", &self.replaced), ("skipped", &self.skipped), ("already there", &self.unchanged)]
            .iter()
            .filter(|(_, l)| !l.is_empty())
            .map(|(what, l)| format!("{} {what}", l.len()))
            .collect();
        if parts.is_empty() {
            parts.push("nothing to import".into());
        }
        let mut s = parts.join(", ");
        if !self.missing_machines.is_empty() {
            s += &format!("; settings kept for machines you do not have: {}", self.missing_machines.join(", "));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::store::{LibraryFile, merged};

    #[test]
    fn export_and_import_with_conflicts() {
        let lib = merged(LibraryFile::default());
        let mat = lib.materials[0].clone();
        let b = bundle(&lib, None, Some(std::slice::from_ref(&mat.id))).unwrap();
        assert_eq!(b.materials.len(), 1);
        assert!(b.machines.iter().any(|m| m.id == "xtool-m2"), "the machines its settings use come along");
        let path = std::env::temp_dir().join(format!("scriber-exchange-{}.scrlib", std::process::id()));
        write(&b, &path).unwrap();
        let back = read(&path).unwrap();
        std::fs::remove_file(&path).ok();

        // The same library: nothing changes.
        let mut same = lib.clone();
        assert_eq!(conflicts(&same, &back), 0);
        let r = import(&mut same, back, OnConflict::Replace);
        assert!(r.added.is_empty() && r.replaced.is_empty());
        assert_eq!(same, lib);

        // A library where the machine and material differ.
        let mut other = lib.clone();
        let mut m2 = other.machine("xtool-m2").unwrap().clone();
        m2.notes = "mine".into();
        other.put_machine(m2);
        other.material_mut(&mat.id).unwrap().notes = "mine".into();
        let b = bundle(&lib, None, Some(std::slice::from_ref(&mat.id))).unwrap();
        assert_eq!(conflicts(&other, &b), 2);
        let mut skip = other.clone();
        import(&mut skip, bundle(&lib, None, Some(std::slice::from_ref(&mat.id))).unwrap(), OnConflict::Skip);
        assert_eq!(skip, other);
        let mut both = other.clone();
        let r = import(&mut both, b, OnConflict::KeepBoth);
        assert_eq!(r.added.len(), 2);
        let copy = both.material(&r.added[1]).unwrap();
        assert!(copy.settings.iter().all(|s| s.machine == r.added[0]), "settings follow the renamed machine");
        let mut replace = other.clone();
        import(&mut replace, bundle(&lib, None, Some(std::slice::from_ref(&mat.id))).unwrap(), OnConflict::Replace);
        assert_eq!(replace.material(&mat.id), lib.material(&mat.id));
    }

    #[test]
    fn missing_machines_are_reported_and_settings_kept() {
        let mut lib = merged(LibraryFile::default());
        let mut mat = lib.materials[0].clone();
        mat.id = "new-one".into();
        mat.settings.iter_mut().for_each(|s| s.machine = "k40".into());
        let b = Bundle { format: FORMAT.into(), version: VERSION, machines: vec![], materials: vec![mat.clone()] };
        let r = import(&mut lib, b, OnConflict::Skip);
        assert_eq!(r.missing_machines, ["k40"]);
        assert_eq!(lib.material("new-one").unwrap().settings, mat.settings);
        assert!(r.summary().contains("k40"));
    }
}
