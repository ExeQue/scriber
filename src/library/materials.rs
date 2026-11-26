//! Materials and their laser settings per machine, laser module and operation.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// What the laser does, as far as settings go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Cut through.
    Cut,
    /// Mark a thin line.
    Score,
    /// Burn a line into the surface.
    Engrave,
    /// Engrave a whole area, line by line.
    Fill,
}

impl Operation {
    pub const ALL: [Operation; 4] = [Operation::Cut, Operation::Score, Operation::Engrave, Operation::Fill];

    pub fn label(self) -> &'static str {
        match self {
            Operation::Cut => "Cut",
            Operation::Score => "Score",
            Operation::Engrave => "Engrave line",
            Operation::Fill => "Fill engrave",
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
    /// Power in percent (0–100).
    pub power: f64,
    /// Speed in mm/s.
    pub speed: f64,
    #[serde(default = "one")]
    pub passes: u32,
    /// Fill only: distance between engraved lines in mm (lines per cm = 10 / this).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_interval: Option<f64>,
    /// Fill only: engrave in both directions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bidirectional: Option<bool>,
}

fn one() -> u32 {
    1
}

impl Setting {
    /// e.g. "100 % · 8 mm/s · 2×".
    pub fn hint(&self) -> String {
        let mut s = format!("{} % · {} mm/s · {}×", self.power, self.speed, self.passes);
        if let Some(i) = self.line_interval.filter(|i| *i > 0.0) {
            s += &format!(" · {i} mm ({:.0} lines/cm)", 10.0 / i);
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Material {
    /// Unique key, e.g. "basswood-plywood-3mm".
    pub id: String,
    pub name: String,
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
    /// The setting for an operation on a machine; without a laser module, the first module that has one.
    pub fn setting(&self, machine: &str, laser: Option<&str>, op: Operation) -> Option<&Setting> {
        self.settings.iter().find(|s| s.machine == machine && s.operation == op && laser.is_none_or(|l| s.laser == l))
    }

    /// Add or replace the setting for the same machine, laser module and operation.
    pub fn put(&mut self, setting: Setting) {
        match self.settings.iter_mut().find(|s| s.machine == setting.machine && s.laser == setting.laser && s.operation == setting.operation) {
            Some(s) => *s = setting,
            None => self.settings.push(setting),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if self.id.is_empty() || self.name.trim().is_empty() {
            return Err("a material needs an id and a name".into());
        }
        if self.thickness < 0.0 {
            return Err(format!("{}: thickness cannot be negative", self.name));
        }
        self.settings.iter().try_for_each(|s| s.check().map_err(|e| format!("{}: {e}", self.name)))
    }
}
