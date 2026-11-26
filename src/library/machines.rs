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
    /// xTool Studio's name for this module, e.g. "laser_blue_10W"; missing = derived (`xtool_default`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xtool_laser: Option<String>,
}

impl Laser {
    /// xTool's module string from the kind and power: "laser_<blue|red|co2|fiber>_<W>W".
    pub fn xtool_default(&self) -> String {
        let source = match self.kind {
            LaserKind::Diode => "blue",
            LaserKind::Infrared => "red",
            LaserKind::Co2 => "co2",
            LaserKind::Fiber => "fiber",
        };
        format!("laser_{source}_{}W", self.power_w)
    }

    /// The module string to write for xTool Studio: the set one, else the derived one.
    pub fn xtool_module(&self) -> String {
        self.xtool_laser.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| self.xtool_default())
    }
}

/// How xTool Studio knows a machine.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct XtoolIds {
    /// xTool's device code, e.g. "JS002" (M2).
    pub device: String,
    /// xTool's short name, e.g. "M2"; missing = the machine's model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
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
    /// xTool Studio's ids for this machine (.xs export, importing xTool materials).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xtool: Option<XtoolIds>,
}

impl Machine {
    /// The work area for a laser module (the machine's when the module has none or is unknown).
    pub fn area(&self, laser: Option<&str>) -> [f64; 2] {
        laser.and_then(|l| self.laser(l)).and_then(|l| l.area).unwrap_or([self.width, self.height])
    }

    /// xTool's device code, when set.
    pub fn xtool_device(&self) -> Option<&str> {
        self.xtool.as_ref().map(|x| x.device.trim()).filter(|d| !d.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xtool_ids_derive_override_and_old_files() {
        // A machine file from before the xTool fields still loads.
        let old: Machine = serde_json::from_str(r#"{"id":"k","name":"K","width":300,"height":200,"lasers":[{"id":"ir","name":"IR","kind":"infrared","power_w":3}]}"#).unwrap();
        assert_eq!((old.xtool_device(), old.lasers[0].xtool_laser.as_deref()), (None, None));
        assert_eq!(old.lasers[0].xtool_module(), "laser_red_3W");
        let mut l = old.lasers[0].clone();
        (l.kind, l.power_w) = (LaserKind::Diode, 10.0);
        assert_eq!(l.xtool_module(), "laser_blue_10W");
        l.xtool_laser = Some("laser_custom".into());
        assert_eq!(l.xtool_module(), "laser_custom");
        l.xtool_laser = Some(" ".into());
        assert_eq!(l.xtool_module(), "laser_blue_10W", "an empty override means derived");
        let m2: Machine = serde_json::from_str(include_str!("../../machines/xtool-m2.json")).unwrap();
        assert_eq!((m2.xtool_device(), m2.xtool.as_ref().unwrap().name.as_str()), (Some("JS002"), "M2"));
        assert_eq!(m2.lasers.iter().map(Laser::xtool_module).collect::<Vec<_>>(), ["laser_blue_10W", "laser_blue_20W", "laser_blue_40W", "laser_red_3W"]);
    }
}
