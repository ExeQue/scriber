//! The JSON files of an `.xs` project, shaped like the ones xTool Studio 1.9.11 writes (docs/xs-format.md).

use serde_json::{Map, Value, json};

/// xTool's processing types, in the order xTool lists them in `profiles.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Score (a line).
    Line,
    /// Engrave an area.
    Fill,
    Cut,
    /// Image engraving; its profile is only written when the project has an image.
    Bitmap,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Line, Kind::Fill, Kind::Cut];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Line => "VECTOR_ENGRAVING",
            Kind::Fill => "FILL_VECTOR_ENGRAVING",
            Kind::Cut => "VECTOR_CUTTING",
            Kind::Bitmap => "BITMAP_ENGRAVING",
        }
    }

    /// xTool's default values for this type (from `profiles.json` of a new project).
    pub fn values(self, laser: &str) -> Map<String, Value> {
        let v = match self {
            Kind::Line => json!({"power": 1, "speed": 20, "repeat": 1, "laser": laser, "kerfDistance": 0, "processHead": "LASER", "enableKerf": false,
                "powerCutoffType": "FOLLOW_ACTUALLY_POWER", "powerCutoffValue": 3, "airPump": 15}),
            Kind::Fill => json!({"power": 1, "speed": 80, "repeat": 1, "laser": laser, "density": 100, "dpiX": 254, "dpiY": 254, "needGapNumDensity": true,
                "bitmapScanMode": "zMode", "processHead": "LASER", "scanAngle": 0, "angleType": 2, "crossAngle": false, "outlineTrace": false,
                "powerCutoffType": "FOLLOW_ACTUALLY_POWER", "powerCutoffValue": 3, "airPump": 15, "ramp": false, "ramp_length": 0.5,
                "ramp_min_power": 1, "ramp_outer_edge": false, "fill_type": "regular", "offset_fill_direction": "IN_TO_OUT"}),
            Kind::Cut => json!({"power": 1, "speed": 16, "repeat": 1, "laser": laser, "cuttingDrop": false, "sinkingMethod": "one",
                "firstCuttingDropValue": 1, "cuttingDropValue": 1, "descentIntervalDescent": 1, "descentPerStep": 1, "enableBreakPoint": false,
                "breakPointGenMode": "auto", "breakPointSize": 0.5, "breakPointMode": "count", "breakPointCount": 2, "breakPointDistance": 100,
                "breakPointPower": 0, "kerfDistance": 0, "enableKerf": false, "enableOverCut": false, "overCutDistance": 0.5, "processHead": "LASER",
                "powerCutoffType": "FOLLOW_ACTUALLY_POWER", "powerCutoffValue": 15, "airPump": 100}),
            Kind::Bitmap => json!({"power": 1, "speed": 80, "repeat": 1, "laser": laser, "density": 100, "dpiX": 254, "dpiY": 254,
                "bitmapScanMode": "zMode", "bitmapMode": "grayscale", "processHead": "LASER", "powerCutoffType": "FOLLOW_ACTUALLY_POWER",
                "powerCutoffValue": 3, "airPump": 15}),
        };
        let Value::Object(m) = v else { unreachable!() };
        m
    }
}

pub fn project(id: &str, canvas: &str, device: &str, name: &str, now: u64, cover: bool) -> Value {
    let mut p = json!({"__v2__": true, "version": "2.0.0",
        "schemaMeta": {"schemaVersion": "2.1.0", "format": "directory", "migratedFrom": "1.0", "migratedAt": now},
        "projectId": id, "projectTraceID": id, "projectName": name, "activeCanvasId": canvas, "activeDeviceId": device,
        "versionInfo": {"source": "pc", "appVersion": "1.9.11", "savedAt": now, "ua": "Scriber", "minRequiredVersion": "2.6.0",
            "appMinRequiredVersion": "", "webMinRequiredVersion": ""},
        "created": now, "modify": now, "modules": {"canvases": [canvas], "devices": [device]},
        "cover": "resources/project-cover.png", "customProjectData": {"projectTraceID": id}});
    if !cover {
        p.as_object_mut().unwrap().remove("cover");
    }
    p
}

pub const COVER_META: &str = r#"{"ref":"resources/project-cover.png","metadata":{"kind":"image","source":{"type":"workspace","value":"project-cover.png"},"mimeType":"image/png"}}"#;

pub fn canvas(id: &str, layers: Map<String, Value>, displays: usize) -> Value {
    json!({"id": id, "title": "{panel}1", "hidden": false, "layerData": layers, "groupData": {},
        "batchData": {"entryType": "BATCH_TEMPLATE", "bindings": []},
        "extendInfo": {"version": "2.16.5", "minCanvasVersion": "0.0.0", "displayProcessConfigMap": {}, "rulerPluginData": {"rulerGuide": []}, "type": "2d"},
        "chunkLayout": {"displayCount": displays, "chunkCount": 1, "chunkIndexes": [0]}})
}

/// `devices/device-<code>-1.json`: one canvas on the flat bed, no material (as a new project).
/// `ignored`: ids of displays xTool shows but does not process (its "no output").
#[allow(clippy::too_many_arguments)]
pub fn device(code: &str, ext_name: &str, watts: f64, canvas: &str, profiles: &[String], ignored: Vec<Value>, patches: Map<String, Value>, bindings: Vec<Value>) -> Value {
    json!({"id": format!("{code}-1"), "deviceCode": code, "extId": code, "extName": ext_name, "power": [watts],
        "processing": {canvas: {"id": canvas, "activeMode": "LASER_PLANE", "modes": {"LASER_PLANE": {
            "ignoredDisplayIds": ignored,
            "data": {"material": 0, "thickness": null, "perimeter": null, "diameter": null, "processPath": "auto", "pathPlanning": "auto",
                "fillPlanning": "separate", "scanDirection": "topToBottom", "enableRemoveCommonEdge": false, "lineSpacing": 0.025,
                "focalLen": null, "focalLength": null, "distance": null},
            "profileRefs": profiles, "patches": patches, "bindings": bindings}}}}})
}

/// One element's outline: the path string (or where it was moved to) and its shape facts.
pub struct Outline {
    /// `("dPath", "M…")` or `("vectorRef", {…})`.
    pub path: (&'static str, Value),
    pub bbox: [f64; 4],
    pub closed: bool,
    pub compound: bool,
}

/// A `PATH` display in world coordinates (identity transform), with every field xTool writes.
pub fn display(id: &str, tag: &str, rgb: [u8; 3], z: usize, area: bool, o: Outline) -> Value {
    let color = (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32;
    let r6 = |v: f64| (v * 1e6).round() / 1e6 + 0.0; // + 0.0 turns -0 into 0
    let [x, y, w, h] = o.bbox.map(r6);
    let mut d = json!({"id": id, "name": null, "type": "PATH", "x": x, "y": y, "angle": 0,
        "scale": {"x": 1, "y": 1}, "skew": {"x": 0, "y": 0}, "pivot": {"x": 0, "y": 0}, "localSkew": {"x": 0, "y": 0},
        "offsetX": 0, "offsetY": 0, "lockRatio": true, "isClosePath": o.closed, "zOrder": z, "groupTags": [], "groupTag": super::uuid(),
        "layerTag": tag, "layerColor": tag, "visible": true, "originColor": tag, "enableTransform": true, "visibleState": true, "lockState": false,
        "resourceOrigin": "", "customData": {"tabBreaks": {}, "startPoint": {}}, "rootComponentId": "", "minCanvasVersion": "0.0.0", "alpha": 1});
    // Two halves: one json! of all fields exceeds the macro's recursion limit.
    let rest = json!({"fill": {"paintType": "color", "visible": area, "color": color, "alpha": 1},
        "stroke": {"paintType": "color", "visible": !area, "color": color, "alpha": 1, "width": 0.3, "cap": "butt", "join": "miter", "miterLimit": 4, "alignment": 0.5},
        "effects": [], "transform": [1, 0, 0, 1, 0, 0], "width": w, "height": h, "isFill": area, "lineColor": color, "fillColor": tag,
        "points": [], "fillRule": if o.compound { "evenodd" } else { "nonzero" }, "graphicX": 0, "graphicY": 0,
        "isCompoundPath": o.compound, "pathCornerData": null});
    let Value::Object(rest) = rest else { unreachable!() };
    d.as_object_mut().unwrap().extend(rest);
    d[o.path.0] = o.path.1;
    d
}

/// A `BITMAP` display: the PNG at `resource`, `px` pixels drawn at `bbox` (x, y, w, h in mm, unrotated).
pub fn bitmap(id: &str, tag: &str, rgb: [u8; 3], z: usize, resource: &str, px: [u32; 2], bbox: [f64; 4]) -> Value {
    let outline = Outline { path: ("dPath", Value::Null), bbox, closed: false, compound: false };
    let mut d = display(id, tag, rgb, z, true, outline);
    let m = d.as_object_mut().unwrap();
    for k in ["points", "fillRule", "graphicX", "graphicY", "isCompoundPath", "pathCornerData", "dPath"] {
        m.remove(k);
    }
    let (sx, sy) = (bbox[2] / px[0] as f64, bbox[3] / px[1] as f64);
    let rest = json!({"type": "BITMAP", "offsetX": bbox[0], "offsetY": bbox[1], "scale": {"x": sx, "y": sy}, "transform": [sx, 0, 0, sy, bbox[0], bbox[1]],
        "resourcePath": resource, "format": "png", "originWidth": px[0], "originHeight": px[1], "currentUrl": "", "opacity": 1, "mask": null,
        "modifyData": {}, "warpLocalOffset": null, "filterList": [], "filterList_V2": [], "originAutoAdjust": null, "autoStrength": 0,
        "dpi": {"dpiX": 25.4 / sx, "dpiY": 25.4 / sy}, "grayValue": [0, 255], "sharpness": 50, "brightness": 0, "contrast": 0, "saturation": 0,
        "temperature": 0, "tone": 0, "colorInverted": false, "colorInvertedFillTransparent": false, "colorInvertedTransparentColor": "black", "isGray": false,
        "filterAttrsMap": {"emboss": {"strength": 5}, "halftone": {"radius": 4, "angle": 45}, "binary": {"threshold": 128}, "sketch": {"strength": 2}, "dot": {"angle": 45, "scale": 14}}});
    let Value::Object(rest) = rest else { unreachable!() };
    m.extend(rest);
    d
}

/// The sidecar of an image resource.
pub fn resource_meta(name: &str) -> Value {
    json!({"ref": format!("resources/{name}"), "metadata": {"kind": "image", "source": {"type": "workspace", "value": name}}})
}
