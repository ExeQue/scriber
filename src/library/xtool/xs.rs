//! xTool Studio `.xs` projects (zip): `profiles.json` holds each process's base values and
//! `devices/device-<code>-<n>.json` the material choice and per-process overrides. No material name
//! inside, so the file name is used.

use super::{XtoolMaterial, XtoolSetting, category, dedupe, num, operation};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

/// "laser_blue_10W" → ("blue", 10).
fn laser(s: &str) -> Option<(String, f64)> {
    let mut parts = s.split('_').skip(1);
    let source = parts.next()?.to_lowercase();
    let watts = parts.next()?.trim_end_matches(['W', 'w']).parse().ok()?;
    Some((source, watts))
}

fn json(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Value, String> {
    let mut text = String::new();
    zip.by_name(name).map_err(|e| format!("{name}: {e}"))?.read_to_string(&mut text).map_err(|e| format!("{name}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))
}

/// The material settings in an `.xs` project: one entry per device and canvas that has any.
pub fn read_project(path: &Path) -> Result<Vec<XtoolMaterial>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{} is not an xTool Studio project: {e}", path.display()))?;
    let profiles = json(&mut zip, "profiles.json")?;
    let devices: Vec<String> = zip.file_names().filter_map(Result::ok).filter(|n| n.starts_with("devices/") && n.ends_with(".json")).map(|n| n.into_owned()).collect();
    let stem = path.file_stem().map_or("xTool project".into(), |s| s.to_string_lossy().into_owned());
    let mut out = Vec::new();
    for name in devices {
        let device = json(&mut zip, &name)?;
        let code = device["deviceCode"].as_str().unwrap_or_default().to_string();
        for canvas in device["processing"].as_object().into_iter().flat_map(|c| c.values()) {
            for mode in canvas["modes"].as_object().into_iter().flat_map(|m| m.values()) {
                let data = &mode["data"];
                let mut list = Vec::new();
                let ignored: Vec<&Value> = mode["ignoredDisplayIds"].as_array().into_iter().flatten().collect();
                let bindings: Vec<&Value> = mode["bindings"].as_array().into_iter().flatten().collect();
                for (id, patch) in mode["patches"].as_object().into_iter().flatten() {
                    // A patch only "no output" elements use (xTool keeps their binding) is no material setting.
                    let users: Vec<&Value> = bindings
                        .iter()
                        .filter(|b| b["patchIds"].as_array().is_some_and(|p| p.iter().any(|p| p == id.as_str())))
                        .flat_map(|b| b["displayIds"].as_array().into_iter().flatten())
                        .collect();
                    if !users.is_empty() && users.iter().all(|d| ignored.contains(d)) {
                        continue;
                    }
                    let profile = &profiles["profiles"][patch["profileId"].as_str().unwrap_or_default()];
                    let Some(op) = operation(profile["processingType"].as_str().unwrap_or_default()) else { continue };
                    let mut values = profile["values"].as_object().cloned().unwrap_or_default();
                    values.extend(patch["overrides"].as_object().cloned().unwrap_or_default());
                    let values = Value::Object(values);
                    let Some((source, watts)) = values["laser"].as_str().and_then(laser) else { continue };
                    let (Some(power), Some(speed)) = (num(&values["power"]), num(&values["speed"]).filter(|s| *s > 0.0)) else { continue };
                    list.push((
                        false,
                        XtoolSetting {
                            operation: op,
                            source,
                            watts,
                            power: power.clamp(0.0, 100.0),
                            speed,
                            passes: num(&values["repeat"]).map_or(1, |r| (r as u32).max(1)),
                            // density is lines per cm (density 100 goes with dpi 254 = 0.1 mm).
                            lines_per_cm: (op == super::Operation::Engrave).then(|| num(&values["density"])).flatten().filter(|d| *d > 0.0),
                        },
                    ));
                }
                let settings = dedupe(list);
                if settings.is_empty() {
                    continue;
                }
                let n = out.len();
                out.push(XtoolMaterial {
                    key: format!("{}@{code}", num(&data["material"]).map_or("project".into(), |m| (m as i64).to_string())),
                    name: if n == 0 { stem.clone() } else { format!("{stem} ({})", n + 1) },
                    thickness: num(&data["thickness"]).map_or(0.0, |t| (t * 100.0).round() / 100.0),
                    category: category("", ""),
                    device: code.clone(),
                    settings,
                });
            }
        }
    }
    if out.is_empty() {
        return Err(format!("{} has no laser settings that Scriber can use (cut, score or fill engrave)", path.display()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Operation;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn reads_a_project() {
        let path = std::env::temp_dir().join(format!("scriber-xs-{}-Test board.xs", std::process::id()));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let profiles = json!({"profiles": {
            "p:cut": {"processingType": "VECTOR_CUTTING", "values": {"power": 1, "speed": 16, "repeat": 1, "laser": "laser_blue_10W"}},
            "p:fill": {"processingType": "FILL_VECTOR_ENGRAVING", "values": {"power": 1, "speed": 80, "repeat": 1, "laser": "laser_blue_10W", "density": 100}},
            "p:img": {"processingType": "BITMAP_ENGRAVING", "values": {"power": 30, "speed": 80, "laser": "laser_blue_10W"}}}});
        let device = json!({"deviceCode": "JS002", "processing": {"c1": {"modes": {"LASER_PLANE": {
            "data": {"material": 7, "thickness": 3.0601},
            "patches": {
                "a": {"profileId": "p:cut", "overrides": {"power": 100, "speed": 4, "repeat": 2}},
                "b": {"profileId": "p:fill", "overrides": {"power": 80, "speed": 200}},
                "c": {"profileId": "p:img", "overrides": {}}}}}}}});
        zip.start_file("profiles.json", opts).unwrap();
        zip.write_all(profiles.to_string().as_bytes()).unwrap();
        zip.start_file("devices/device-JS002-1.json", opts).unwrap();
        zip.write_all(device.to_string().as_bytes()).unwrap();
        zip.finish().unwrap();
        let list = read_project(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(list.len(), 1);
        let m = &list[0];
        assert!(m.name.ends_with("Test board"));
        assert_eq!((m.key.as_str(), m.device.as_str(), m.thickness), ("7@JS002", "JS002", 3.06));
        assert_eq!(m.settings.len(), 2, "bitmap engraving is skipped");
        let cut = m.settings.iter().find(|s| s.operation == Operation::Cut).unwrap();
        assert_eq!((cut.power, cut.speed, cut.passes, cut.watts, cut.source.as_str()), (100.0, 4.0, 2, 10.0, "blue"));
        let fill = m.settings.iter().find(|s| s.operation == Operation::Engrave).unwrap();
        assert_eq!((fill.power, fill.speed, fill.lines_per_cm), (80.0, 200.0, Some(100.0)));
        assert!(read_project(Path::new("/nonexistent.xs")).is_err());
    }
}
