//! Materials and their laser settings per machine, laser module and operation.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// What the laser does, as far as settings go (the item ops that reach the laser).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Cut through.
    Cut,
    /// Mark a thin line.
    Score,
    /// Engrave a whole area, line by line. Older libraries had separate "engrave" (line) and "fill" keys.
    #[serde(alias = "fill")]
    Engrave,
}

impl Operation {
    pub const ALL: [Operation; 3] = [Operation::Cut, Operation::Score, Operation::Engrave];

    pub fn label(self) -> &'static str {
        match self {
            Operation::Cut => "Cut",
            Operation::Score => "Score",
            Operation::Engrave => "Engrave",
        }
    }
}

/// How to run one operation on one machine's laser module.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Setting {
    /// Machine id.
    pub machine: String,
    /// Laser module id of that machine.
    pub laser: String,
    pub operation: Operation,
    /// Profile name, e.g. "Light" or "Photo": a material can have several settings per machine,
    /// laser and operation. "" (missing in older files) is the default one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Power in percent (0–100).
    pub power: f64,
    /// Speed in mm/s.
    pub speed: f64,
    #[serde(default = "one")]
    pub passes: u32,
    /// Engrave only: distance between engraved lines in mm. People see it as lines per cm = 10 / this (0.1 mm = 100 lines/cm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_interval: Option<f64>,
    /// Engrave only: engrave in both directions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bidirectional: Option<bool>,
}

fn one() -> u32 {
    1
}

impl Setting {
    /// A default profile: full power, 10 mm/s, one pass (engrave: 100 lines per cm, one way).
    pub fn new(machine: &str, laser: &str, operation: Operation) -> Self {
        let engrave = operation == Operation::Engrave;
        Setting {
            machine: machine.into(),
            laser: laser.into(),
            operation,
            name: String::new(),
            power: 100.0,
            speed: 10.0,
            passes: 1,
            line_interval: engrave.then_some(0.1),
            bidirectional: engrave.then_some(false),
        }
    }

    /// Whether it is for this machine, laser and operation (any profile).
    pub fn is_for(&self, machine: &str, laser: &str, op: Operation) -> bool {
        self.machine == machine && self.laser == laser && self.operation == op
    }

    /// e.g. "100 % · 8 mm/s · 2×".
    pub fn hint(&self) -> String {
        let mut s = format!("{} % · {} mm/s · {}×", self.power, self.speed, self.passes);
        if let Some(i) = self.line_interval.filter(|i| *i > 0.0) {
            s += &format!(" · {} lines/cm", (100.0 / i).round() / 10.0);
        }
        if self.bidirectional == Some(true) {
            s += " · both ways";
        }
        s
    }

    pub fn check(&self) -> Result<(), String> {
        if !(0.0..=100.0).contains(&self.power) {
            return Err("power must be between 0 and 100 %".into());
        }
        if self.speed <= 0.0 || self.passes == 0 || self.line_interval.is_some_and(|i| i <= 0.0) {
            return Err("speed, passes and line interval must be larger than 0".into());
        }
        Ok(())
    }
}

/// What kind of material it is, so a long list can be filtered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Paper,
    Wood,
    Plywood,
    Acrylic,
    Leather,
    Fabric,
    Metal,
    Stone,
    Glass,
    #[default]
    Other,
}

impl Category {
    pub const ALL: [Category; 10] = [Category::Paper, Category::Wood, Category::Plywood, Category::Acrylic, Category::Leather, Category::Fabric, Category::Metal, Category::Stone, Category::Glass, Category::Other];

    pub fn label(self) -> &'static str {
        match self {
            Category::Paper => "Paper & card",
            Category::Wood => "Wood",
            Category::Plywood => "Plywood & MDF",
            Category::Acrylic => "Acrylic",
            Category::Leather => "Leather",
            Category::Fabric => "Fabric & felt",
            Category::Metal => "Metal",
            Category::Stone => "Stone & tile",
            Category::Glass => "Glass",
            Category::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Material {
    /// Unique key, e.g. "basswood-plywood-3mm".
    pub id: String,
    pub name: String,
    /// Missing in older files: Other.
    #[serde(default)]
    pub category: Category,
    /// Thickness in mm.
    pub thickness: f64,
    #[serde(default)]
    pub notes: String,
    /// Where the settings come from.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub settings: Vec<Setting>,
}

impl Material {
    /// The default setting for an operation on a machine (the unnamed profile, else the first one);
    /// without a laser module, the first module that has one.
    pub fn setting(&self, machine: &str, laser: Option<&str>, op: Operation) -> Option<&Setting> {
        self.profile(machine, laser, op, None)
    }

    /// The profile with this name, falling back to the default one when there is none by that name.
    pub fn profile(&self, machine: &str, laser: Option<&str>, op: Operation, name: Option<&str>) -> Option<&Setting> {
        let mut list = self.settings.iter().filter(|s| s.machine == machine && s.operation == op && laser.is_none_or(|l| s.laser == l));
        let laser = list.next()?.laser.clone();
        let list: Vec<&Setting> = self.settings.iter().filter(|s| s.is_for(machine, &laser, op)).collect();
        name.and_then(|n| list.iter().find(|s| s.name == n)).or_else(|| list.iter().find(|s| s.name.is_empty())).or(list.first()).copied()
    }

    /// The profile names for a machine, laser and operation, the default first ("" when unnamed).
    pub fn profile_names(&self, machine: &str, laser: Option<&str>, op: Operation) -> Vec<String> {
        let Some(default) = self.setting(machine, laser, op) else { return Vec::new() };
        let mut names = vec![default.name.clone()];
        names.extend(self.settings.iter().filter(|s| s.is_for(machine, &default.laser, op) && s.name != default.name).map(|s| s.name.clone()));
        names
    }

    /// Add or replace the setting for the same machine, laser module, operation and profile name.
    pub fn put(&mut self, setting: Setting) {
        match self.settings.iter_mut().find(|s| s.is_for(&setting.machine, &setting.laser, setting.operation) && s.name == setting.name) {
            Some(s) => *s = setting,
            None => self.settings.push(setting),
        }
    }

    /// Old libraries could hold both an engrave-line and a fill setting, which are both Engrave now:
    /// keep one per machine, laser and operation, preferring the old fill one (it has a line interval).
    pub fn merge_old_engrave(&mut self) {
        let mut settings = std::mem::take(&mut self.settings);
        settings.sort_by_key(|s| s.line_interval.is_some());
        settings.into_iter().for_each(|s| self.put(s));
    }

    pub fn check(&self) -> Result<(), String> {
        if self.id.is_empty() || self.name.trim().is_empty() {
            return Err("a material needs an id and a name".into());
        }
        if self.thickness < 0.0 {
            return Err(format!("{}: thickness cannot be negative", self.name));
        }
        for (i, s) in self.settings.iter().enumerate() {
            s.check().map_err(|e| format!("{}: {e}", self.name))?;
            if self.settings[..i].iter().any(|t| t.is_for(&s.machine, &s.laser, s.operation) && t.name.trim() == s.name.trim()) {
                let name = if s.name.trim().is_empty() { "unnamed (default)".to_string() } else { format!("\"{}\"", s.name.trim()) };
                return Err(format!("{}: two {} profiles named {name} for the same laser", self.name, s.operation.label().to_lowercase()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_shows_lines_per_cm() {
        let mut s = Setting { power: 30.0, speed: 200.0, bidirectional: Some(true), ..Setting::new("m", "l", Operation::Engrave) };
        assert_eq!(s.hint(), "30 % · 200 mm/s · 1× · 100 lines/cm · both ways");
        s.line_interval = Some(0.07);
        assert_eq!(s.hint(), "30 % · 200 mm/s · 1× · 142.9 lines/cm · both ways");
    }

    #[test]
    fn named_profiles() {
        let engrave = |name: &str, power: f64| Setting { name: name.into(), power, ..Setting::new("m", "l", Operation::Engrave) };
        let mut m = Material { id: "oak".into(), name: "Oak".into(), category: Category::Wood, thickness: 3.0, notes: String::new(), source: String::new(), settings: vec![engrave("Photo", 20.0)] };
        assert_eq!(m.setting("m", None, Operation::Engrave).unwrap().name, "Photo", "without an unnamed one the first is the default");
        m.put(engrave("", 30.0));
        m.put(engrave("Dark", 60.0));
        m.put(engrave("Dark", 70.0));
        assert_eq!(m.settings.len(), 3, "put replaces the same name");
        assert_eq!(m.setting("m", Some("l"), Operation::Engrave).unwrap().power, 30.0, "the unnamed one is the default");
        assert_eq!(m.profile("m", None, Operation::Engrave, Some("Dark")).unwrap().power, 70.0);
        assert_eq!(m.profile("m", None, Operation::Engrave, Some("gone")).unwrap().power, 30.0, "an unknown name falls back to the default");
        assert_eq!(m.profile_names("m", None, Operation::Engrave), ["", "Photo", "Dark"]);
        assert!(m.check().is_ok());
        m.settings.push(engrave("Dark", 10.0));
        assert!(m.check().unwrap_err().contains("two engrave profiles named \"Dark\""));
        // Old files have no names and load as the default profile.
        let old: Setting = serde_json::from_str(r#"{"machine":"m","laser":"l","operation":"cut","power":100,"speed":8}"#).unwrap();
        assert_eq!((old.name.as_str(), old.passes), ("", 1));
        assert!(!serde_json::to_string(&old).unwrap().contains("name"));
    }
}
