//! Machine profiles: a laser cutter with its work area and laser modules.

use super::Operation;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LaserKind {
    Diode,
    Co2,
    Fiber,
    Infrared,
}

impl LaserKind {
    pub const ALL: [LaserKind; 4] = [LaserKind::Diode, LaserKind::Co2, LaserKind::Fiber, LaserKind::Infrared];

    pub fn label(self) -> &'static str {
        match self {
            LaserKind::Diode => "Diode (blue)",
            LaserKind::Co2 => "CO2",
            LaserKind::Fiber => "Fiber",
            LaserKind::Infrared => "Infrared",
        }
    }
}

/// One laser module of a machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Laser {
    /// Unique within the machine, e.g. "diode-20w".
    pub id: String,
    pub name: String,
    pub kind: LaserKind,
    /// Laser power in watts.
    pub power_w: f64,
    /// What it can do.
    #[serde(default)]
    pub ops: Vec<Operation>,
    /// Work area [width, height] in mm when it differs from the machine's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<[f64; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Machine {
    /// Unique key, e.g. "xtool-m2".
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub model: String,
    /// Work area width in mm.
    pub width: f64,
    /// Work area height in mm.
    pub height: f64,
    #[serde(default)]
    pub lasers: Vec<Laser>,
    #[serde(default)]
    pub notes: String,
    /// Where the numbers come from, and which ones are unverified.
    #[serde(default)]
    pub source: String,
}

impl Machine {
    /// The work area for a laser module (the machine's when the module has none or is unknown).
    pub fn area(&self, laser: Option<&str>) -> [f64; 2] {
        laser.and_then(|l| self.laser(l)).and_then(|l| l.area).unwrap_or([self.width, self.height])
    }

    pub fn laser(&self, id: &str) -> Option<&Laser> {
        self.lasers.iter().find(|l| l.id == id)
    }

    pub fn check(&self) -> Result<(), String> {
        let area_ok = |[w, h]: [f64; 2]| w > 0.0 && h > 0.0;
        if self.id.is_empty() || self.name.trim().is_empty() {
            return Err("a machine needs an id and a name".into());
        }
        if !area_ok([self.width, self.height]) || self.lasers.iter().any(|l| !l.area.is_none_or(area_ok)) {
            return Err(format!("{}: the work area must be larger than 0", self.name));
        }
        if self.lasers.iter().any(|l| l.power_w <= 0.0) {
            return Err(format!("{}: laser power must be larger than 0", self.name));
        }
        Ok(())
    }
}
