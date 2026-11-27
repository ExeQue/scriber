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

/// One way the machine's software engraves an image: greyscale or a dither.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImageMode {
    /// Scriber's id, stored on layers, e.g. "bayer".
    pub id: String,
    /// What people see, e.g. "Bayer".
    pub name: String,
    /// What the export writes for the machine's software, e.g. "Bayer" in an xTool `.xs` project.
    pub value: String,
}

/// A machine's image modes: the name of a built-in set (`machines/image-modes/<name>.json`, shared by
/// the machines of one vendor's software) or the machine's own list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ImageModes {
    Set(String),
    List(Vec<ImageMode>),
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
    /// How its software can engrave images (greyscale, dithers); the first is the default. Missing =
    /// none to choose from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_modes: Option<ImageModes>,
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

    /// The image modes it offers, the default first; empty when it has none or names an unknown set.
    pub fn image_modes(&self) -> Vec<ImageMode> {
        match &self.image_modes {
            Some(ImageModes::List(list)) => list.clone(),
            Some(ImageModes::Set(name)) => super::image_mode_set(name).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// The mode with this id, else the default (None when the machine has no modes).
    pub fn image_mode(&self, id: Option<&str>) -> Option<ImageMode> {
        let modes = self.image_modes();
        modes.iter().find(|m| Some(m.id.as_str()) == id).or(modes.first()).cloned()
    }

    /// The same machine with a set name replaced by its list, so it works in another Scriber (shared
    /// libraries) whatever sets that one has.
    pub fn inlined(&self) -> Machine {
        let mut m = self.clone();
        if let Some(ImageModes::Set(name)) = &m.image_modes
            && let Some(list) = super::image_mode_set(name)
        {
            m.image_modes = Some(ImageModes::List(list));
        }
        m
    }

    /// The reverse of `inlined`: a list that is exactly a built-in set becomes that set's name again,
    /// so an imported machine matches the one it was shared from and follows later fixes to the set.
    pub fn with_set_names(mut self) -> Machine {
        if let Some(ImageModes::List(list)) = &self.image_modes
            && let Some(name) = super::image_mode_set_names().into_iter().find(|n| super::image_mode_set(n).as_ref() == Some(list))
        {
            self.image_modes = Some(ImageModes::Set(name));
        }
        self
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
        // An unknown set name is allowed (a newer Scriber may have it); the pre-flight check says so.
        if let Some(ImageModes::List(list)) = &self.image_modes {
            check_modes(list).map_err(|e| format!("{}: {e}", self.name))?;
        }
        Ok(())
    }
}

/// Image modes need an id and a name each, and no id twice.
pub fn check_modes(list: &[ImageMode]) -> Result<(), String> {
    if list.iter().any(|m| m.id.trim().is_empty() || m.name.trim().is_empty()) {
        return Err("every image mode needs an id and a name".into());
    }
    if let Some(m) = list.iter().enumerate().find(|(k, m)| list[..*k].iter().any(|o| o.id == m.id)).map(|(_, m)| m) {
        return Err(format!("image mode \"{}\" is listed twice", m.id));
    }
    Ok(())
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

    #[test]
    fn image_modes_come_from_a_set_or_the_machine() {
        let m2: Machine = serde_json::from_str(include_str!("../../machines/xtool-m2.json")).unwrap();
        assert_eq!(m2.image_modes, Some(ImageModes::Set("xtool-studio".into())));
        let ids: Vec<String> = m2.image_modes().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["grayscale", "jarvis", "bayer", "floyd", "stucki", "atkinson", "sierra", "passthrough"]);
        assert_eq!(m2.image_mode(Some("floyd")).unwrap().value, "Floyd");
        assert_eq!(m2.image_mode(Some("passthrough")).unwrap().value, "passthrough", "xTool writes this one in lower case");
        assert_eq!(m2.image_mode(Some("gone")).unwrap().id, "grayscale", "unknown = the default, the first");
        assert_eq!(m2.image_mode(None).unwrap().id, "grayscale");
        // Shared machines carry the list itself; saved as a list, read back as one.
        let inlined = m2.inlined();
        assert!(matches!(&inlined.image_modes, Some(ImageModes::List(l)) if l.len() == 8));
        let back: Machine = serde_json::from_str(&serde_json::to_string(&inlined).unwrap()).unwrap();
        assert_eq!((back.image_modes(), back.image_modes == inlined.image_modes), (m2.image_modes(), true));
        assert_eq!(back.with_set_names(), m2, "a list that is a built-in set gets its name back on import");
        let mut own = inlined.clone();
        if let Some(ImageModes::List(l)) = &mut own.image_modes {
            l.pop();
        }
        assert_eq!(own.clone().with_set_names(), own, "any other list stays a list");
        // An unknown set gives no modes but still loads and checks.
        let mut newer = m2.clone();
        newer.image_modes = Some(ImageModes::Set("from-the-future".into()));
        assert_eq!((newer.image_modes().len(), newer.image_mode(None), newer.inlined().image_modes), (0, None, newer.image_modes.clone()));
        newer.check().unwrap();
        // A machine's own list is checked.
        let mode = |id: &str| ImageMode { id: id.into(), name: id.into(), value: id.into() };
        newer.image_modes = Some(ImageModes::List(vec![mode("a"), mode("a")]));
        assert!(newer.check().unwrap_err().contains("twice"));
        newer.image_modes = Some(ImageModes::List(vec![mode("")]));
        assert!(newer.check().is_err());
    }
}
