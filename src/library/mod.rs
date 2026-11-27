//! Machine profiles and the material library: which laser settings to use for which material on
//! which machine. Built-in machines ship in `machines/` (no material presets; the materials in
//! `test_materials/` are test fixtures); the user's own machines and materials (and their edits of
//! built-ins) live in one JSON file in the config folder.

mod machines;
mod materials;
pub mod exchange;
pub mod sources;
pub mod store;
pub mod xtool;

pub use machines::{ImageMode, Laser, LaserKind, Machine};
pub use materials::{Category, Material, Operation, Setting};

use parking_lot::{Mutex, MutexGuard};
use std::sync::LazyLock;

#[allow(dead_code)]
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
    /// What the user's library sits on: built-ins and subscribed sources. Never saved.
    pub base: Base,
}

/// Where a machine or material comes from. Not saved: worked out from the base on every load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    /// The user's own, or their edit of a built-in.
    #[default]
    User,
    Builtin,
    /// A subscribed library repository (read-only; duplicate it to change it).
    Source { id: String, name: String },
}

impl Origin {
    pub fn is_source(&self) -> bool {
        matches!(self, Origin::Source { .. })
    }

    /// "from <source name>" for people; empty for the user's own and built-ins.
    pub fn label(&self) -> String {
        match self {
            Origin::Source { name, .. } => format!("from {name}"),
            _ => String::new(),
        }
    }
}

/// Built-in machines and materials with the enabled sources' on top, each with its origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Base {
    pub machines: Vec<(Origin, Machine)>,
    pub materials: Vec<(Origin, Material)>,
    /// Id clashes between sources and files that could not be read, for people.
    pub problems: Vec<String>,
}

impl Base {
    pub fn builtin() -> Self {
        Base {
            machines: builtin_machines().into_iter().map(|m| (Origin::Builtin, m)).collect(),
            materials: builtin_materials().into_iter().map(|m| (Origin::Builtin, m)).collect(),
            problems: Vec::new(),
        }
    }
}

fn parse_all<T: serde::de::DeserializeOwned>(files: &[(&str, &str)]) -> Vec<T> {
    // The files are checked by a unit test, so a bad one cannot ship.
    files.iter().map(|(name, json)| serde_json::from_str(json).unwrap_or_else(|e| panic!("built-in {name}: {e}"))).collect()
}

pub fn builtin_machines() -> Vec<Machine> {
    parse_all(builtin::MACHINES)
}

/// A built-in set of image modes by name (`machines/image-modes/<name>.json`).
pub fn image_mode_set(name: &str) -> Option<Vec<ImageMode>> {
    let (_, json) = builtin::IMAGE_MODES.iter().find(|(file, _)| file.strip_suffix(".json") == Some(name))?;
    Some(serde_json::from_str(json).unwrap_or_else(|e| panic!("built-in image modes {name}: {e}")))
}

/// The names of the built-in image mode sets.
pub fn image_mode_set_names() -> Vec<String> {
    builtin::IMAGE_MODES.iter().filter_map(|(file, _)| file.strip_suffix(".json")).map(String::from).collect()
}

/// Scriber ships no material presets: people build their own library. Tests use fixtures.
#[cfg(not(test))]
pub fn builtin_materials() -> Vec<Material> {
    Vec::new()
}

#[cfg(test)]
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

    /// The named profile (the default one when the name is missing or unknown).
    pub fn profile(&self, machine: &str, laser: Option<&str>, material: &str, op: Operation, name: Option<&str>) -> Option<&Setting> {
        self.material(material)?.profile(machine, laser, op, name)
    }

    /// Add a machine, or replace the one with the same id (a deleted built-in with that id is back).
    pub fn put_machine(&mut self, m: Machine) {
        self.removed_machines.retain(|r| *r != m.id);
        put(&mut self.machines, m, |m| &m.id);
    }

    pub fn put_material(&mut self, m: Material) {
        self.removed_materials.retain(|r| *r != m.id);
        put(&mut self.materials, m, |m| &m.id);
    }

    /// Delete a material, as the library window does: the user's own and built-ins, not a source's.
    pub fn delete_material(&mut self, id: &str) -> Result<Material, String> {
        let m = self.material(id).cloned().ok_or(format!("no material with id {id}"))?;
        read_only(&self.material_origin(id), "material", &m.name)?;
        self.remove_material(id);
        Ok(m)
    }

    /// Delete one profile of a material's setting (name "" = the default profile).
    pub fn delete_setting(&mut self, material: &str, machine: &str, laser: &str, op: Operation, name: &str) -> Result<Setting, String> {
        let m = self.material(material).ok_or(format!("no material with id {material}"))?;
        read_only(&self.material_origin(material), "material", &m.name)?;
        let m = self.material_mut(material).unwrap();
        let i = m.settings.iter().position(|s| s.is_for(machine, laser, op) && s.name.trim() == name.trim());
        let what = if name.trim().is_empty() { "default".to_string() } else { format!("\"{}\"", name.trim()) };
        let i = i.ok_or(format!("{} has no {what} {} profile for {machine} / {laser}", m.name, op.label().to_lowercase()))?;
        Ok(m.settings.remove(i))
    }

    /// Delete a machine; refused while materials have settings for it unless `force` (the settings
    /// stay, so restoring the machine brings them back). Built-ins stay deleted until restored.
    pub fn delete_machine(&mut self, id: &str, force: bool) -> Result<(Machine, Vec<String>), String> {
        let m = self.machine(id).cloned().ok_or(format!("no machine with id {id}"))?;
        read_only(&self.machine_origin(id), "machine", &m.name)?;
        let users: Vec<String> = self.materials.iter().filter(|mat| mat.settings.iter().any(|s| s.machine == id)).map(|mat| mat.name.clone()).collect();
        if !users.is_empty() && !force {
            return Err(format!("{} still has settings in: {}. Delete those first, or pass force: true to keep them (unused) and delete the machine", m.name, users.join(", ")));
        }
        self.remove_machine(id);
        Ok((m, users))
    }

    /// Bring back a deleted built-in (or source) machine.
    pub fn restore_machine(&mut self, id: &str) -> Result<Machine, String> {
        if !self.removed_machines.iter().any(|r| r == id) {
            return Err(format!("no deleted machine with id {id}"));
        }
        let m = self.base.machines.iter().find(|(_, m)| m.id == id).map(|(_, m)| m.clone()).ok_or(format!("{id} is no longer in Scriber or a subscribed source"))?;
        self.put_machine(m.clone());
        Ok(m)
    }

    pub fn is_builtin_machine(&self, id: &str) -> bool {
        self.base.machines.iter().any(|(o, m)| m.id == id && *o == Origin::Builtin)
    }

    pub fn remove_machine(&mut self, id: &str) {
        self.machines.retain(|m| m.id != id);
        if self.base.machines.iter().any(|(_, m)| m.id == id) && !self.removed_machines.iter().any(|r| r == id) {
            self.removed_machines.push(id.into());
        }
    }

    pub fn remove_material(&mut self, id: &str) {
        self.materials.retain(|m| m.id != id);
        if self.base.materials.iter().any(|(_, m)| m.id == id) && !self.removed_materials.iter().any(|r| r == id) {
            self.removed_materials.push(id.into());
        }
    }

    /// Where the machine comes from: the base's origin while it is unchanged, else the user's.
    pub fn machine_origin(&self, id: &str) -> Origin {
        origin(&self.base.machines, self.machine(id))
    }

    pub fn material_origin(&self, id: &str) -> Origin {
        origin(&self.base.materials, self.material(id))
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

/// Items from a subscribed source are read-only (the library window disables Delete for them too).
fn read_only(origin: &Origin, noun: &str, name: &str) -> Result<(), String> {
    match origin {
        Origin::Source { name: source, .. } => Err(format!("the {noun} \"{name}\" comes from the library source \"{source}\" and is read-only; turn the source off (set_library_source enabled: false) to hide it")),
        _ => Ok(()),
    }
}

fn origin<T: PartialEq>(base: &[(Origin, T)], item: Option<&T>) -> Origin {
    item.and_then(|i| base.iter().find(|(_, b)| b == i)).map(|(o, _)| o.clone()).unwrap_or_default()
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
    LIBRARY.lock()
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

/// Read the subscribed sources again (after a sync, subscribe, unsubscribe or toggle); the user's part stays.
pub fn reload_sources() {
    let base = sources::base();
    let mut lib = get();
    *lib = store::rebase(&lib, base);
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
            if let Some(machines::ImageModes::Set(name)) = &m.image_modes {
                assert!(image_mode_set(name).is_some(), "{file}: unknown image mode set {name}");
            }
        }
        for (file, _) in builtin::IMAGE_MODES {
            let list = image_mode_set(file.strip_suffix(".json").unwrap()).unwrap();
            assert!(!list.is_empty(), "{file}: empty");
            machines::check_modes(&list).unwrap_or_else(|e| panic!("{file}: {e}"));
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
    fn preview_fields_are_optional_in_every_format() {
        let old = serde_json::json!({"id": "kraft", "name": "Kraft", "category": "paper", "thickness": 0.3, "notes": "", "source": "",
            "settings": [{"machine": "xtool-m2", "laser": "diode-10w", "operation": "engrave", "power": 40.0, "speed": 140.0, "passes": 1, "line_interval": 0.1}]});
        let m: Material = serde_json::from_value(old.clone()).unwrap();
        assert_eq!((m.burn_colour, m.settings[0].preview_opacity), (None, None));
        assert_eq!(serde_json::to_value(&m).unwrap(), old, "saved as before, without the new keys");
        let new_keys = |text: &str| text.contains("preview_opacity") || text.contains("burn_colour");

        let dir = std::env::temp_dir().join(format!("scriber-preview-fields-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut lib = store::merged(store::LibraryFile::default());
        lib.put_material(m.clone());
        let ids = ["kraft".to_string()];

        let path = dir.join("library.json");
        store::save(&lib, &path).unwrap();
        assert!(!new_keys(&std::fs::read_to_string(&path).unwrap()), "library.json");
        assert_eq!(store::load(&path).unwrap().material("kraft"), Some(&m));

        let path = dir.join("kraft.scrlib");
        exchange::write(&exchange::bundle(&lib, Some(&[][..]), Some(&ids[..])).unwrap(), &path).unwrap();
        assert!(!new_keys(&std::fs::read_to_string(&path).unwrap()), ".scrlib");
        assert_eq!(exchange::read(&path).unwrap().materials, [m.clone()]);

        let repo = dir.join("repo");
        sources::export_repository(&lib, &repo, Some(&[][..]), Some(&ids[..]), Some("Kraft"), None).unwrap();
        assert!(!new_keys(&std::fs::read_to_string(repo.join(".scriber/materials/kraft.json")).unwrap()), "repository");
        assert_eq!(sources::read_repo(&repo).unwrap().materials, [m.clone()]);

        let mut with = m;
        with.burn_colour = Some([60, 30, 10]);
        with.settings[0].preview_opacity = Some(0.5);
        let text = serde_json::to_string(&with).unwrap();
        assert!(text.contains(r#""burn_colour":[60,30,10]"#) && text.contains(r#""preview_opacity":0.5"#), "{text}");
        assert_eq!(serde_json::from_str::<Material>(&text).unwrap(), with);
        with.settings[0].preview_opacity = Some(1.5);
        assert!(with.check().is_err(), "preview_opacity is 0-1");
        let _ = std::fs::remove_dir_all(&dir);
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
    fn deleting_follows_the_library_window() {
        let friends = Origin::Source { id: "o-r".into(), name: "Friends".into() };
        let mut base = Base::builtin();
        let shared = Material { id: "shared".into(), name: "Shared".into(), ..base.materials[0].1.clone() };
        let theirs = Machine { id: "their-laser".into(), name: "Their laser".into(), ..base.machines[0].1.clone() };
        base.materials.push((friends.clone(), shared));
        base.machines.push((friends, theirs));
        let mut lib = store::merged_with(store::LibraryFile::default(), base.clone());

        // Source items are read-only.
        assert!(lib.delete_material("shared").unwrap_err().contains("\"Friends\""));
        assert!(lib.delete_setting("shared", "xtool-m2", "diode-10w", Operation::Cut, "").unwrap_err().contains("read-only"));
        assert!(lib.delete_machine("their-laser", true).unwrap_err().contains("Friends"));

        // One profile goes, the others stay.
        let cut = |name: &str| Setting { name: name.into(), ..Setting::new("xtool-m2", "diode-10w", Operation::Cut) };
        let mine = Material { id: "mine".into(), name: "3mm MDF (2 passes)".into(), settings: vec![cut(""), cut("Fast")], ..lib.materials[0].clone() };
        lib.put_material(mine);
        assert_eq!(lib.delete_setting("mine", "xtool-m2", "diode-10w", Operation::Cut, "").unwrap().name, "");
        assert_eq!(lib.material("mine").unwrap().settings, [cut("Fast")]);
        assert!(lib.delete_setting("mine", "xtool-m2", "diode-10w", Operation::Cut, "").is_err());

        // A machine with materials is refused, listing them; force keeps their settings.
        assert!(lib.delete_machine("xtool-m2", false).unwrap_err().contains("3mm MDF (2 passes)"));
        let (_, kept) = lib.delete_machine("xtool-m2", true).unwrap();
        assert!(kept.contains(&"3mm MDF (2 passes)".to_string()) && lib.machine("xtool-m2").is_none());
        assert_eq!(lib.material("mine").unwrap().settings.len(), 1);
        // A deleted built-in stays deleted after saving and loading, until restored.
        let reload = |lib: &Library| store::merged_with(store::user_part(lib), base.clone());
        assert!(reload(&lib).machine("xtool-m2").is_none());
        assert!(lib.is_builtin_machine("xtool-m2"));
        lib.restore_machine("xtool-m2").unwrap();
        let back = reload(&lib);
        assert!(back.machine("xtool-m2").is_some() && back.removed_machines.is_empty());
        assert!(lib.restore_machine("xtool-m2").is_err(), "not deleted");

        lib.delete_material("mine").unwrap();
        assert!(reload(&lib).material("mine").is_none());
    }

    #[test]
    fn new_ids_are_unique() {
        let lib = store::merged(store::LibraryFile::default());
        assert_eq!(lib.new_id("xTool M2"), "xtool-m2-2");
        assert_eq!(lib.new_id("My K40!"), "my-k40");
        assert_eq!(lib.new_id("  "), "item");
    }
}
