//! Machine profiles and the material library: which laser settings to use for which material on
//! which machine. Built-in machines and materials ship in `machines/` and `materials/`; the user's own
//! (and their edits of built-ins) live in one JSON file in the config folder.

mod machines;
mod materials;
pub mod store;

pub use machines::{Laser, LaserKind, Machine};
pub use materials::{Material, Operation, Setting};

use std::sync::{LazyLock, Mutex, MutexGuard};

mod builtin {
    include!(concat!(env!("OUT_DIR"), "/builtin.rs"));
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub machines: Vec<Machine>,
    pub materials: Vec<Material>,
    /// Built-in machines and materials the user deleted (so they do not come back).
    pub removed_machines: Vec<String>,
    pub removed_materials: Vec<String>,
}

fn parse_all<T: serde::de::DeserializeOwned>(files: &[(&str, &str)]) -> Vec<T> {
    // The files are checked by a unit test, so a bad one cannot ship.
    files.iter().map(|(name, json)| serde_json::from_str(json).unwrap_or_else(|e| panic!("built-in {name}: {e}"))).collect()
}

pub fn builtin_machines() -> Vec<Machine> {
    parse_all(builtin::MACHINES)
}

pub fn builtin_materials() -> Vec<Material> {
    parse_all(builtin::MATERIALS)
}

impl Library {
    pub fn machine(&self, id: &str) -> Option<&Machine> {
        self.machines.iter().find(|m| m.id == id)
    }

    pub fn material(&self, id: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.id == id)
    }

    pub fn material_mut(&mut self, id: &str) -> Option<&mut Material> {
        self.materials.iter_mut().find(|m| m.id == id)
    }

    /// The setting for `op` with this material on this machine (and laser module, when given).
    pub fn setting(&self, machine: &str, laser: Option<&str>, material: &str, op: Operation) -> Option<&Setting> {
        self.material(material)?.setting(machine, laser, op)
    }

    /// Add a machine, or replace the one with the same id.
    pub fn put_machine(&mut self, m: Machine) {
        put(&mut self.machines, m, |m| &m.id);
    }

    pub fn put_material(&mut self, m: Material) {
        put(&mut self.materials, m, |m| &m.id);
    }

    pub fn remove_machine(&mut self, id: &str) {
        self.machines.retain(|m| m.id != id);
        if builtin_machines().iter().any(|m| m.id == id) {
            self.removed_machines.push(id.into());
        }
    }

    pub fn remove_material(&mut self, id: &str) {
        self.materials.retain(|m| m.id != id);
        if builtin_materials().iter().any(|m| m.id == id) {
            self.removed_materials.push(id.into());
        }
    }

    pub fn check(&self) -> Result<(), String> {
        self.machines.iter().try_for_each(Machine::check)?;
        self.materials.iter().try_for_each(Material::check)
    }

    /// An id based on `name` that no machine or material uses yet.
    pub fn new_id(&self, name: &str) -> String {
        let base: String = name.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
        let base = if base.is_empty() { "item".to_string() } else { base };
        let taken = |id: &str| self.machine(id).is_some() || self.material(id).is_some();
        (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|id| !taken(id)).unwrap()
    }
}

fn put<T>(list: &mut Vec<T>, item: T, id: fn(&T) -> &String) {
    match list.iter_mut().find(|o| id(o) == id(&item)) {
        Some(o) => *o = item,
        None => list.push(item),
    }
}

// ---------- the library of this app (shared by the window and MCP) ----------

static LIBRARY: LazyLock<Mutex<Library>> = LazyLock::new(|| Mutex::new(store::load_or_backup(&store::path())));

/// The current library. Do not hold the lock across a call to `update`.
pub fn get() -> MutexGuard<'static, Library> {
    LIBRARY.lock().unwrap()
}

/// Change the library and save it; nothing changes when the result is invalid or cannot be saved.
pub fn update<T>(f: impl FnOnce(&mut Library) -> Result<T, String>) -> Result<T, String> {
    let mut lib = get().clone();
    let out = f(&mut lib)?;
    lib.check()?;
    store::save(&lib, &store::path()).map_err(|e| format!("could not save the library: {e}"))?;
    *get() = lib;
    Ok(out)
}

/// The settings to use for an operation, for exporters (e.g. a future xTool `.xs` export).
#[allow(dead_code, reason = "for the coming xTool exporter")]
pub fn settings_for(machine: &str, laser: Option<&str>, material: &str, op: Operation) -> Option<Setting> {
    get().setting(machine, laser, material, op).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_file_parses() {
        let machines = builtin_machines();
        assert!(!machines.is_empty());
        for (m, (file, _)) in machines.iter().zip(builtin::MACHINES) {
            assert_eq!(format!("{}.json", m.id), *file, "the file name must be the machine id");
            assert!(m.width > 0.0 && m.height > 0.0, "{file}: work area");
            m.check().unwrap();
        }
        let lib = Library { machines, materials: builtin_materials(), ..Library::default() };
        for (m, (file, _)) in lib.materials.iter().zip(builtin::MATERIALS) {
            assert_eq!(format!("{}.json", m.id), *file);
            assert!(!m.source.is_empty(), "{file}: built-in settings must cite a source");
            m.check().unwrap();
            for s in &m.settings {
                let machine = lib.machine(&s.machine).unwrap_or_else(|| panic!("{file}: unknown machine {}", s.machine));
                assert!(machine.laser(&s.laser).is_some(), "{file}: unknown laser {}", s.laser);
            }
        }
    }

    #[test]
    fn settings_lookup() {
        let lib = store::merged(store::LibraryFile::default());
        let s = lib.setting("xtool-m2", Some("diode-10w"), "black-acrylic-3mm", Operation::Cut).unwrap();
        assert_eq!((s.power, s.speed, s.passes), (100.0, 4.0, 2));
        assert_eq!(s.hint(), "100 % · 4 mm/s · 2×");
        // Without a module: the first one that has a setting.
        assert_eq!(lib.setting("xtool-m2", None, "black-acrylic-3mm", Operation::Cut).unwrap().laser, "diode-10w");
        assert!(lib.setting("xtool-m2", Some("ir-3w"), "black-acrylic-3mm", Operation::Cut).is_none());
        assert!(lib.setting("xtool-m2", None, "black-acrylic-3mm", Operation::Engrave).is_none());
        assert_eq!(lib.machine("xtool-m2").unwrap().area(Some("diode-40w")), [426.0, 312.0]);
    }

    #[test]
    fn new_ids_are_unique() {
        let lib = store::merged(store::LibraryFile::default());
        assert_eq!(lib.new_id("xTool M2"), "xtool-m2-2");
        assert_eq!(lib.new_id("My K40!"), "my-k40");
        assert_eq!(lib.new_id("  "), "item");
    }
}
