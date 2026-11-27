//! Import material settings from xTool Studio: its local material cache (Chromium IndexedDB) and
//! `.xs` project files. Read-only: xTool's folder is copied to a temp folder first and only the
//! material catalogue and parameter stores are decoded; nothing else in it is read.

mod leveldb;
mod v8;
mod xs;

use super::{Category, Laser, LaserKind, Library, Material, Operation, Setting};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub use xs::read_project;

/// One operation for one laser module, as xTool stores it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct XtoolSetting {
    pub operation: Operation,
    /// xTool's laser source: "blue" (diode), "red" (infrared), "co2", "uv", …
    pub source: String,
    pub watts: f64,
    pub power: f64,
    pub speed: f64,
    pub passes: u32,
    /// Engrave only.
    pub lines_per_cm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct XtoolMaterial {
    /// `<material id>@<device code>`, unique per machine type.
    pub key: String,
    pub name: String,
    pub thickness: f64,
    pub category: Category,
    /// xTool's device code, e.g. "JS002" (M2).
    pub device: String,
    pub settings: Vec<XtoolSetting>,
}

/// The machine in `lib` whose xTool device code is `device` (set in the machine's definition).
/// Devices without one are listed but cannot be imported.
pub fn machine_for<'a>(lib: &'a Library, device: &str) -> Option<&'a str> {
    lib.machines.iter().find(|m| m.xtool_device() == Some(device)).map(|m| m.id.as_str())
}

/// Names of device codes seen in xTool's cache that no machine definition has yet.
const DEVICE_NAMES: [(&str, &str); 1] = [("ZH009", "xTool M1 Ultra")];

/// A device code's name: the machine's (the library's, else a built-in's), else a known name, else the code.
pub fn device_name(lib: &Library, device: &str) -> String {
    let named = |ms: &[super::Machine]| ms.iter().find(|m| m.xtool_device() == Some(device)).map(|m| m.name.clone());
    named(&lib.machines)
        .or_else(|| named(&super::builtin_machines()))
        .or_else(|| DEVICE_NAMES.iter().find(|(d, _)| *d == device).map(|(_, n)| n.to_string()))
        .unwrap_or_else(|| format!("xTool device {device}"))
}

/// One xTool material with its parameter sets, one per xTool machine type that has settings for it.
#[derive(Clone, Debug, PartialEq)]
pub struct XtoolGroup {
    /// xTool's material id.
    pub id: String,
    pub name: String,
    pub thickness: f64,
    pub category: Category,
    pub sets: Vec<XtoolMaterial>,
}

/// The parameter sets grouped by material, in the list's order.
pub fn group(list: Vec<XtoolMaterial>) -> Vec<XtoolGroup> {
    let mut out: Vec<XtoolGroup> = Vec::new();
    for x in list {
        let id = x.key.split('@').next().unwrap_or_default().to_string();
        match out.iter_mut().find(|g| g.id == id) {
            Some(g) => g.sets.push(x),
            None => out.push(XtoolGroup { id, name: x.name.clone(), thickness: x.thickness, category: x.category, sets: vec![x] }),
        }
    }
    out
}

impl XtoolGroup {
    /// The machines in `lib` it can be imported for, with their parameter set; `first` (the document's machine) first.
    pub fn targets<'a>(&'a self, lib: &'a Library, first: Option<&str>) -> Vec<(&'a str, &'a XtoolMaterial)> {
        let mut out: Vec<_> = self.sets.iter().filter_map(|x| machine_for(lib, &x.device).filter(|m| !modules(lib, m, x).is_empty()).map(|m| (m, x))).collect();
        out.sort_by_key(|(m, _)| Some(*m) != first);
        out
    }

    /// Why it cannot be imported, e.g. "for xTool F1 — not in your machines".
    pub fn reason(&self, lib: &Library) -> String {
        let mut names: Vec<String> = self.sets.iter().map(|x| device_name(lib, &x.device)).collect();
        names.dedup();
        let shown = if names.len() > 3 { format!("{} and {} more", names[..3].join(", "), names.len() - 3) } else { names.join(", ") };
        if self.sets.iter().any(|x| machine_for(lib, &x.device).is_some()) {
            format!("for {shown} — none of its laser modules match yours")
        } else {
            format!("for {shown} — not in your machines")
        }
    }
}

/// The laser modules of `machine` that `x` has settings for, in the machine's order, each with its settings.
pub fn modules<'a, 'b>(lib: &'a Library, machine: &str, x: &'b XtoolMaterial) -> Vec<(&'a Laser, Vec<&'b XtoolSetting>)> {
    let Some(m) = lib.machine(machine) else { return Vec::new() };
    m.lasers
        .iter()
        .filter_map(|l| {
            let list: Vec<&XtoolSetting> = x.settings.iter().filter(|s| laser_for(lib, machine, s).as_deref() == Some(l.id.as_str())).collect();
            (!list.is_empty()).then_some((l, list))
        })
        .collect()
}

fn kind_of(source: &str) -> Option<LaserKind> {
    match source {
        "blue" => Some(LaserKind::Diode),
        "red" | "ir" | "infrared" => Some(LaserKind::Infrared),
        "co2" => Some(LaserKind::Co2),
        "fiber" => Some(LaserKind::Fiber),
        _ => None,
    }
}


/// xTool processing types: vector cut → cut, vector engrave (a line) → score, fill → engrave.
/// Bitmap (image) engraving is skipped: its numbers are tuned for dithered photos, not filled shapes.
fn operation(processing_type: &str) -> Option<Operation> {
    match processing_type {
        "VECTOR_CUTTING" => Some(Operation::Cut),
        "VECTOR_ENGRAVING" => Some(Operation::Score),
        "FILL_VECTOR_ENGRAVING" => Some(Operation::Engrave),
        _ => None,
    }
}

fn category(first: &str, second: &str) -> Category {
    let t = format!("{first} {second}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["plywood", "mdf"]) {
        Category::Plywood
    } else if has(&["leather"]) {
        Category::Leather
    } else if has(&["acrylic"]) {
        Category::Acrylic
    } else if has(&["paper", "card"]) {
        Category::Paper
    } else if has(&["wood", "bamboo", "cork"]) {
        Category::Wood
    } else if has(&["fabric", "felt", "cotton", "denim", "linen", "cloth"]) {
        Category::Fabric
    } else if has(&["metal", "steel", "aluminum", "aluminium", "copper", "brass", "titanium"]) {
        Category::Metal
    } else if has(&["stone", "slate", "tile", "ceramic", "marble", "granite"]) {
        Category::Stone
    } else if has(&["glass"]) {
        Category::Glass
    } else {
        Category::Other
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
}

fn code(v: &Value, field: &str) -> String {
    match &v[field] {
        Value::Object(o) => o.get("code").and_then(|c| c.as_str().map(str::to_string).or_else(|| num(c).map(|n| n.to_string()))).unwrap_or_default(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// One xTool parameter entry (`machineParas[]` in the cache).
fn setting(p: &Value) -> Option<XtoolSetting> {
    let speed = num(&p["speed"])?;
    let lines = num(&p["density"]).or_else(|| code(p, "density").parse().ok()).filter(|d| *d > 0.0);
    let operation = operation(&code(p, "processingType"))?;
    Some(XtoolSetting {
        operation,
        source: code(p, "laserSource"),
        watts: code(p, "laserPower").parse().ok()?,
        power: num(&p["power"])?.clamp(0.0, 100.0),
        speed: (speed > 0.0).then_some(speed)?,
        passes: num(&p["repeat"]).map_or(1, |r| (r as u32).max(1)),
        lines_per_cm: if operation == Operation::Engrave { lines } else { None },
    })
}

/// One setting per operation and module, in xTool's order with its default variant first.
fn dedupe(mut list: Vec<(bool, XtoolSetting)>) -> Vec<XtoolSetting> {
    list.sort_by_key(|(default, _)| !default);
    let mut out: Vec<XtoolSetting> = Vec::new();
    for (_, s) in list {
        if !out.iter().any(|o| o.operation == s.operation && o.source == s.source && o.watts == s.watts) {
            out.push(s);
        }
    }
    out.sort_by_key(|s| Operation::ALL.iter().position(|o| *o == s.operation));
    out
}

/// Build the materials from decoded catalogue entries and parameter sets (one per material and device).
fn parse(catalogue: &[Value], params: &[Value]) -> Vec<XtoolMaterial> {
    let mut out: Vec<XtoolMaterial> = params
        .iter()
        .filter_map(|p| {
            let id = num(&p["materialId"])? as i64;
            let device = p["deviceCode"].as_str()?.to_string();
            let paras = p["machineParas"].as_array()?;
            let settings = dedupe(paras.iter().filter(|m| code(m, "deviceMode").is_empty() || code(m, "deviceMode") == device).filter_map(|m| Some((m["isDefault"] == true, setting(m)?))).collect());
            if settings.is_empty() {
                return None;
            }
            let info = catalogue.iter().find(|m| num(&m["id"]).map(|i| i as i64) == Some(id));
            let name = info.and_then(|m| {
                let n = &m["name"];
                n["en"].as_str().or(n.as_str()).or_else(|| n.as_object()?.values().find_map(Value::as_str)).filter(|s| !s.trim().is_empty())
            });
            Some(XtoolMaterial {
                key: format!("{id}@{device}"),
                name: name.map_or_else(|| format!("xTool material #{id}"), |n| n.trim().to_string()),
                thickness: info.and_then(|m| num(&m["thickness"])).unwrap_or(0.0).max(0.0),
                category: info.map_or(Category::Other, |m| category(m["categoryCode"].as_str().unwrap_or(""), m["secondCategoryCode"].as_str().unwrap_or(""))),
                device,
                settings,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.key.cmp(&b.key)));
    out
}

/// Where xTool Studio's data folder may be: `$SCRIBER_XTOOL_DIR`, else the config folder
/// (`~/.config`, `%APPDATA%` (Roaming) or `~/Library/Application Support`) under either spelling,
/// and under WSL the Windows users' `AppData/Roaming`.
pub fn studio_dirs() -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("SCRIBER_XTOOL_DIR") {
        return vec![dir.into()];
    }
    let mut roots: Vec<PathBuf> = dirs::config_dir().into_iter().collect();
    if std::env::var_os("WSL_DISTRO_NAME").is_some()
        && let Ok(users) = std::fs::read_dir("/mnt/c/Users")
    {
        roots.extend(users.flatten().map(|u| u.path().join("AppData/Roaming")));
    }
    roots.iter().flat_map(|r| [r.join("xTool Studio"), r.join("xTool_Studio")]).collect()
}

const CACHE: &str = "IndexedDB/atomm_renderer_0.indexeddb.leveldb";
/// The IndexedDB database holding the material stores (its name, not the folder's), and its two stores.
const DATABASE: &str = "ATOMM_MATERIAL";
const CATALOGUE_STORE: u8 = 1;
const PARAMS_STORE: u8 = 2;

/// A temp copy of xTool's cache folder, removed when dropped.
struct TempCopy(PathBuf);

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Decode a Chromium IndexedDB string key part: varint length, then UTF-16BE.
fn idb_string(b: &[u8]) -> Option<(String, usize)> {
    let mut r = leveldb::Bytes::new(b);
    let n = r.varint()? as usize;
    let units: Vec<u16> = r.take(n.checked_mul(2)?)?.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    Some((String::from_utf16_lossy(&units), r.i))
}

/// The materials in xTool Studio's cache at `dir` (its `…indexeddb.leveldb` folder).
pub fn read_cache_at(dir: &Path) -> Result<Vec<XtoolMaterial>, String> {
    if !dir.is_dir() {
        return Err(format!("xTool Studio's material cache was not found at {}. Is xTool Studio installed, and has it shown its material library at least once?", dir.display()));
    }
    // Copy first: xTool may be running and writing; we never touch its folder.
    let tmp = TempCopy(std::env::temp_dir().join(format!("scriber-xtool-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())));
    std::fs::create_dir_all(&tmp.0).map_err(|e| e.to_string())?;
    // A file xTool holds locked (Windows, while it runs) is skipped; newer data may then be missing.
    let mut failed = None;
    for path in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()) {
        if path.extension().is_some_and(|e| e == "ldb" || e == "log")
            && let Err(e) = std::fs::copy(&path, tmp.0.join(path.file_name().unwrap()))
        {
            failed = Some(e);
        }
    }
    let locked = |what: &str| match &failed {
        Some(e) => format!("{what} (some cache files could not be read: {e}; close xTool Studio and try again)"),
        None => what.to_string(),
    };
    // The database id: from the metadata entry naming the database (global prefix 00 00 00 00, type 201).
    let meta = leveldb::read_dir(&tmp.0, &|k| k.starts_with(&[0, 0, 0, 0, 201])).map_err(|e| e.to_string())?;
    let db = meta
        .iter()
        .find_map(|(k, v)| {
            let (_origin, used) = idb_string(&k[5..])?;
            let (name, _) = idb_string(&k[5 + used..])?;
            let id = leveldb::Bytes::new(v).varint()?;
            (name == DATABASE).then_some(id)
        })
        .and_then(|id| u8::try_from(id).ok())
        .ok_or_else(|| locked("xTool Studio's cache has no material database (unknown xTool Studio version?)"))?;
    // Only the catalogue and parameter stores' records (prefix byte 0 = one-byte ids, index 1 = records).
    let records = leveldb::read_dir(&tmp.0, &|k| k.len() > 4 && k[..2] == [0, db] && (k[2] == CATALOGUE_STORE || k[2] == PARAMS_STORE) && k[3] == 1).map_err(|e| e.to_string())?;
    let (mut catalogue, mut params) = (Vec::new(), Vec::new());
    for (k, v) in &records {
        if let Some(value) = v8::decode(v) {
            (if k[2] == CATALOGUE_STORE { &mut catalogue } else { &mut params }).push(value);
        }
    }
    if params.is_empty() {
        return Err(locked("xTool Studio's material cache holds no parameter sets yet: open the materials in xTool Studio first"));
    }
    Ok(parse(&catalogue, &params))
}

/// The materials in xTool Studio's cache: in `dir` (xTool Studio's data folder) when given, else the
/// first data folder found (see `studio_dirs`).
pub fn read_cache(dir: Option<&Path>) -> Result<Vec<XtoolMaterial>, String> {
    let dir = match dir {
        Some(d) => d.to_path_buf(),
        None => {
            let tried = studio_dirs();
            let found = tried.iter().find(|d| d.is_dir()).cloned();
            found.ok_or_else(|| format!("xTool Studio does not seem to be installed for this user (looked in {})", tried.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")))?
        }
    };
    read_cache_at(&dir.join(CACHE))
}

/// Today as YYYY-MM-DD (UTC).
pub fn today() -> String {
    let days = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() / 86_400) as i64;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

/// The laser module id of `machine` for an xTool source and wattage.
fn laser_for(lib: &Library, machine: &str, s: &XtoolSetting) -> Option<String> {
    let kind = kind_of(&s.source)?;
    lib.machine(machine)?.lasers.iter().find(|l| l.kind == kind && (l.power_w - s.watts).abs() < 0.5).map(|l| l.id.clone())
}

/// The settings per laser module of `machine`, for listings: {"diode-10w": {"cut": "100 % · 4 mm/s · 1×", …}}.
pub fn settings_by_module(lib: &Library, machine: &str, x: &XtoolMaterial) -> serde_json::Map<String, Value> {
    modules(lib, machine, x).into_iter().map(|(l, list)| (l.id.clone(), Value::Object(list.iter().map(|s| (s.operation.label().to_lowercase(), Value::String(s.hint()))).collect()))).collect()
}

impl XtoolSetting {
    /// e.g. "100 % · 4 mm/s · 1×".
    pub fn hint(&self) -> String {
        let mut s = format!("{} % · {} mm/s · {}×", self.power, self.speed, self.passes);
        if let Some(l) = self.lines_per_cm {
            s += &format!(" · {l} lines/cm");
        }
        s
    }
}

/// The `source` text that marks a material imported from xTool entry `key` (used to find it again).
fn source_tag(key: &str) -> String {
    format!("xTool Studio material {key}")
}

/// Add (or with `overwrite`, replace the earlier import of) an xTool material; returns its Scriber id.
/// `origin` says where it came from, e.g. "cache" or "project Coaster.xs". Modules the machine lacks are dropped.
pub fn import(lib: &mut Library, x: &XtoolMaterial, origin: &str, overwrite: bool) -> Result<String, String> {
    let machine = machine_for(lib, &x.device)
        .map(String::from)
        .ok_or_else(|| format!("{}: no machine in the library has the xTool device code {} ({})", x.name, x.device, device_name(lib, &x.device)))?;
    let machine = machine.as_str();
    let settings: Vec<Setting> = x
        .settings
        .iter()
        .filter_map(|s| {
            // One default profile (name "") per module and op.
            Some(Setting {
                power: s.power,
                speed: s.speed,
                passes: s.passes,
                line_interval: s.lines_per_cm.map(|l| 10.0 / l),
                bidirectional: None,
                ..Setting::new(machine, &laser_for(lib, machine, s)?, s.operation)
            })
        })
        .collect();
    if settings.is_empty() {
        return Err(format!("{}: none of its laser modules match {machine}'s", x.name));
    }
    let tag = source_tag(&x.key);
    let earlier = lib.materials.iter().find(|m| m.source.starts_with(&format!("{tag} "))).map(|m| m.id.clone());
    let id = match earlier {
        Some(id) if overwrite => id,
        _ => lib.new_id(&x.name),
    };
    lib.put_material(Material { id: id.clone(), name: x.name.clone(), category: x.category, thickness: x.thickness, color: None, burn_colour: None, notes: String::new(), source: format!("{tag} ({origin}), imported {}", today()), settings });
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn para(kind: &str, source: &str, watts: &str, power: f64, speed: f64, density: Value, default: bool) -> Value {
        json!({"deviceMode": {"code": "JS002"}, "laserSource": {"code": source}, "laserPower": {"code": watts}, "processingType": {"code": kind},
               "power": power, "speed": speed, "repeat": 1, "density": density, "isDefault": default})
    }

    fn sample() -> (Vec<Value>, Vec<Value>) {
        let catalogue = vec![json!({"id": 7.0, "name": {"en": "Test ply 3mm", "de": "Test"}, "thickness": 3, "categoryCode": "Wood", "secondCategoryCode": "PLYWOOD"})];
        let params = vec![
            json!({"materialId": 7.0, "deviceCode": "JS002", "machineParas": [
                para("VECTOR_CUTTING", "blue", "10", 100.0, 4.0, Value::Null, false),
                para("VECTOR_ENGRAVING", "blue", "10", 75.0, 140.0, Value::Null, false),
                para("FILL_VECTOR_ENGRAVING", "blue", "10", 60.0, 300.0, json!({"code": "300"}), false),
                para("FILL_VECTOR_ENGRAVING", "blue", "10", 80.0, 200.0, json!({"code": "100"}), true),
                para("BITMAP_ENGRAVING", "blue", "10", 50.0, 100.0, json!({"code": "100"}), false),
                para("VECTOR_ENGRAVING", "red", "3", 20.0, 50.0, Value::Null, false),
            ]}),
            json!({"materialId": 8, "deviceCode": "ZZ999", "machineParas": [{"deviceMode": {"code": "ZZ999"}, "laserSource": {"code": "blue"}, "laserPower": {"code": "10"}, "processingType": {"code": "VECTOR_CUTTING"}, "power": 90, "speed": 5}]}),
            json!({"materialId": 9, "deviceCode": "JS002", "machineParas": []}),
        ];
        (catalogue, params)
    }

    #[test]
    fn parses_maps_and_dedupes() {
        let (c, p) = sample();
        let list = parse(&c, &p);
        assert_eq!(list.len(), 2, "the empty parameter set is left out");
        let ply = list.iter().find(|m| m.key == "7@JS002").unwrap();
        assert_eq!((ply.name.as_str(), ply.thickness, ply.category), ("Test ply 3mm", 3.0, Category::Plywood));
        assert_eq!(ply.settings.len(), 4, "bitmap skipped, two fill variants become one");
        let engrave = ply.settings.iter().find(|s| s.operation == Operation::Engrave).unwrap();
        assert_eq!((engrave.power, engrave.speed, engrave.lines_per_cm), (80.0, 200.0, Some(100.0)), "the default variant wins");
        let other = list.iter().find(|m| m.key == "8@ZZ999").unwrap();
        assert_eq!(other.name, "xTool material #8", "no catalogue entry");

        let mut lib = super::super::store::merged(super::super::store::LibraryFile::default());
        let ops = settings_by_module(&lib, "xtool-m2", ply);
        assert_eq!(ops["diode-10w"]["cut"], json!("100 % · 4 mm/s · 1×"));
        assert_eq!(ops["diode-10w"]["engrave"], json!("80 % · 200 mm/s · 1× · 100 lines/cm"));
        assert_eq!(ops["ir-3w"], json!({"score": "20 % · 50 mm/s · 1×"}));
        let id = import(&mut lib, ply, "cache", false).unwrap();
        let m = lib.material(&id).unwrap().clone();
        assert_eq!(m.setting("xtool-m2", Some("diode-10w"), Operation::Engrave).unwrap().line_interval, Some(0.1));
        assert_eq!(m.setting("xtool-m2", Some("diode-10w"), Operation::Cut).unwrap().speed, 4.0);
        assert!(m.source.starts_with("xTool Studio material 7@JS002 (cache), imported 20"));
        assert_eq!(import(&mut lib, ply, "cache", true).unwrap(), id, "overwrite replaces the earlier import");
        assert_ne!(import(&mut lib, ply, "cache", false).unwrap(), id, "without overwrite: a second copy");
        assert!(import(&mut lib, other, "cache", false).unwrap_err().contains("ZZ999"));
    }
    /// Parameter sets of one material for several devices are one entry; it can be imported only for
    /// machines in the library whose modules match.
    #[test]
    fn groups_devices_per_material() {
        let (c, mut p) = sample();
        let mut f1 = p[0].clone();
        f1["deviceCode"] = json!("ZX998");
        for m in f1["machineParas"].as_array_mut().unwrap() {
            m["deviceMode"] = json!({"code": "ZX998"});
        }
        p.push(f1);
        let groups = group(parse(&c, &p));
        assert_eq!(groups.len(), 2, "7 on two devices and 8 are two materials");
        let ply = groups.iter().find(|g| g.id == "7").unwrap();
        assert_eq!((ply.name.as_str(), ply.sets.len()), ("Test ply 3mm", 2));
        let other = groups.iter().find(|g| g.id == "8").unwrap();

        let mut lib = super::super::store::merged(super::super::store::LibraryFile::default());
        let targets = ply.targets(&lib, None);
        assert_eq!(targets.iter().map(|(m, x)| (*m, x.device.as_str())).collect::<Vec<_>>(), [("xtool-m2", "JS002")]);
        let modules: Vec<&str> = modules(&lib, "xtool-m2", targets[0].1).iter().map(|(l, _)| l.id.as_str()).collect();
        assert_eq!(modules, ["diode-10w", "ir-3w"]);
        assert!(other.targets(&lib, None).is_empty());
        assert_eq!(other.reason(&lib), "for xTool device ZZ999 — not in your machines");

        lib.machines.retain(|m| m.id != "xtool-m2");
        assert!(ply.targets(&lib, None).is_empty(), "the M2 is not in the library");
        assert_eq!(ply.reason(&lib), "for xTool M2, xTool device ZX998 — not in your machines");
    }

    /// A whole cache folder: database name metadata, both stores, and a record of another store that is never decoded.
    #[test]
    fn reads_a_synthetic_cache() {
        let dir = std::env::temp_dir().join(format!("scriber-xtool-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s16 = |s: &str| [vec![s.encode_utf16().count() as u8], s.encode_utf16().flat_map(u16::to_be_bytes).collect()].concat();
        let name_key = [vec![0, 0, 0, 0, 201], s16("file__0"), s16(DATABASE)].concat();
        let (c, p) = sample();
        let enc = v8::tests::encode;
        let entries: Vec<(Vec<u8>, Vec<u8>)> = vec![
            (name_key, vec![3]),
            ([vec![0, 3, 1, 1], s16("7")].concat(), enc(&c[0])),
            ([vec![0, 3, 2, 1], s16("7@JS002")].concat(), enc(&p[0])),
            ([vec![0, 3, 5, 1], s16("u")].concat(), b"not v8 at all".to_vec()),
        ];
        let mut sorted = entries.clone();
        sorted.sort();
        let refs: Vec<leveldb::tests::RefEntry> =sorted.iter().enumerate().map(|(i, (k, v))| (k.as_slice(), i as u64 + 1, Some(v.as_slice()))).collect();
        std::fs::write(dir.join("000003.ldb"), leveldb::tests::table_file(&refs)).unwrap();
        let list = read_cache_at(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Test ply 3mm");
        assert!(read_cache_at(Path::new("/nonexistent/xtool")).unwrap_err().contains("not found"));
    }

    #[test]
    fn today_is_a_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
    }

}
