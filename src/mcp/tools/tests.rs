//! Replies, file input, curves and SVG import as an agent sees them.

use super::*;
use std::sync::Mutex;

fn server() -> (Server, Shared) {
    let doc: Shared = Arc::new(Mutex::new(Document::new(300.0, 200.0)));
    (Server::new(doc.clone(), Arc::new(|| {})), doc)
}

fn args<T: serde::de::DeserializeOwned>(v: Value) -> Parameters<T> {
    Parameters(serde_json::from_value(v).unwrap())
}

fn reply(r: ToolResult) -> Value {
    let r = r.unwrap();
    let t = r.content[0].as_text().unwrap().text.clone();
    assert_ne!(r.is_error, Some(true), "{t}");
    serde_json::from_str(&t).unwrap()
}

fn error(r: ToolResult) -> String {
    let r = r.unwrap();
    assert_eq!(r.is_error, Some(true));
    r.content[0].as_text().unwrap().text.clone()
}

fn squares(s: &Server, n: usize) -> Vec<u64> {
    let items: Vec<Value> = (0..n).map(|k| json!({"shape": {"kind": "rect", "x": 6.0 * k as f64, "y": 3.0 * (k % 2) as f64, "w": 10, "h": 10}})).collect();
    let out = reply(s.add_items(args(json!({"items": items}))));
    out["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn changes_reply_compactly_unless_verbose() {
    let (s, _) = server();
    let ids = squares(&s, 15);
    let union = reply(s.boolean(args(json!({"ids": ids, "operation": "union"}))));
    let item = &union["items"][0];
    assert_eq!((item["id"].as_u64(), item["op"].as_str(), item["kind"].as_str()), (Some(ids[0]), Some("cut"), Some("path")));
    assert_eq!(item["contours"], 1);
    assert!(item["vertices"].as_u64().unwrap() > 4 && item["bbox"].is_array() && item["rotation"] == 0.0, "{item}");
    assert!(item.get("shape").is_none() && union.to_string().len() < 400, "{union}");

    let full = reply(s.transform_items(args(json!({"ids": [ids[0]], "dx": 1, "verbose": true}))));
    assert!(full["items"][0]["shape"]["contours"].is_array(), "{full}");
}

#[test]
fn get_document_summary_and_ids() {
    let (s, _) = server();
    let ids = squares(&s, 2);
    let full = reply(s.get_document(args(json!({}))));
    assert!(full["items"][0]["shape"].is_object() && full.get("geometry_omitted").is_none());
    let summary = reply(s.get_document(args(json!({"geometry": false}))));
    assert!(summary["items"][0].get("shape").is_none() && summary["items"][1]["vertices"].is_u64());

    // A big document drops the geometry by itself and says how to get it.
    let many: Vec<u64> = (0..300).flat_map(|_| squares(&s, 1)).collect();
    let big = reply(s.get_document(args(json!({}))));
    assert!(big["geometry_omitted"].is_string() && big["items"][0].get("shape").is_none());
    assert!(big.to_string().len() < 100_000);
    let picked = reply(s.get_document(args(json!({"ids": [ids[1], many[0]]}))));
    assert_eq!(picked["items"].as_array().unwrap().len(), 2);
    assert!(picked["items"][0]["shape"].is_object());
}

#[test]
fn add_items_from_a_file() {
    let (s, doc) = server();
    let dir = std::env::temp_dir().join(format!("scriber-items-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("items.json");
    std::fs::write(&file, json!([{"shape": {"kind": "rect", "x": 0, "y": 0, "w": 10, "h": 5}}, {"shape": {"kind": "circle", "cx": 5, "cy": 5, "r": 2}, "op": "score"}]).to_string()).unwrap();
    let out = reply(s.add_items(args(json!({"path": file}))));
    assert_eq!(out["items"][0]["bbox"], json!([0.0, 0.0, 10.0, 5.0]));
    assert_eq!(out["items"][1]["op"], "score");
    assert_eq!(doc.lock().unwrap().s.items.len(), 2);

    std::fs::write(&file, json!({"items": [{"shape": {"kind": "line", "x1": 0, "y1": 0, "x2": 1, "y2": 0}}]}).to_string()).unwrap();
    reply(s.add_items(args(json!({"path": file}))));
    std::fs::write(&file, json!([{"shape": {"kind": "circle", "cx": 5, "cy": 5, "r": 2}}, {"shape": {"kind": "blob"}}]).to_string()).unwrap();
    assert!(error(s.add_items(args(json!({"path": file})))).contains("item 1"));
    assert!(error(s.add_items(args(json!({"path": dir})))).contains("not a regular file"));
    assert!(error(s.add_items(args(json!({"path": dir.join("missing.json")})))).contains("cannot read"));
    let both = json!({"path": file, "items": [{"shape": {"kind": "circle", "cx": 5, "cy": 5, "r": 2}}]});
    assert!(error(s.add_items(args(both))).contains("not both"));
    assert_eq!(doc.lock().unwrap().s.items.len(), 3, "failed calls add nothing");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn curves_become_arcs() {
    let (s, doc) = server();
    // A quarter circle of radius 10 as a cubic, then a quadratic back down, closed.
    let k = 10.0 * 0.552_284_75;
    let curve = json!({"kind": "curve", "start": [0, 10], "closed": true, "segments": [
        {"to": [10, 0], "controls": [[0, 10.0 - k], [10.0 - k, 0]]},
        {"to": [20, 10], "controls": [[20, 0]]},
        {"to": [0, 10]}
    ]});
    let out = reply(s.add_items(args(json!({"items": [{"shape": curve}]}))));
    let item = &out["items"][0];
    assert_eq!(item["kind"], "path");
    let bbox: Vec<f64> = serde_json::from_value(item["bbox"].clone()).unwrap();
    assert!(bbox.iter().zip([0.0, 0.0, 20.0, 10.0]).all(|(a, b)| (a - b).abs() < 0.05), "{bbox:?}");
    let d = doc.lock().unwrap();
    let c = &d.s.items[0].shape.contours()[0];
    assert!(c.closed && c.vertices.iter().any(|v| v.bulge != 0.0) && c.vertices.len() < 20, "{c:?}");
    drop(d);
    let bad = json!({"items": [{"shape": {"kind": "curve", "start": [0, 0], "segments": [{"to": [1, 1], "controls": [[0, 1], [1, 0], [2, 2]]}]}}]});
    assert!(error(s.add_items(args(bad))).contains("control points"));
    let dot = json!({"items": [{"shape": {"kind": "curve", "start": [0, 0], "segments": [{"to": [0, 0]}]}}]});
    assert!(error(s.add_items(args(dot))).contains("away from its start"));
}

#[test]
fn mirrored_copy_keeps_the_original() {
    let (s, doc) = server();
    let tri = json!({"items": [{"shape": {"kind": "polyline", "points": [[0, 0], [10, 0], [0, 5]], "closed": true}}]});
    let id = reply(s.add_items(args(tri)))["ids"][0].as_u64().unwrap();
    let out = reply(s.transform_items(args(json!({"ids": [id], "mirror_x": true, "copy": true, "pivot": [10, 0]}))));
    let copy = out["ids"][0].as_u64().unwrap();
    assert_ne!(copy, id);
    assert_eq!(out["items"][0]["bbox"], json!([10.0, 0.0, 10.0, 5.0]));
    let d = doc.lock().unwrap();
    assert_eq!((d.s.items.len(), d.bbox(&[id])), (2, Some([0.0, 0.0, 10.0, 5.0])));
}

#[test]
fn import_svg_maps_elements_and_keeps_cutters() {
    let (s, doc) = server();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
        <rect id="plate" x="0" y="0" width="60" height="40" fill="none" stroke="red"/>
        <rect id="cutter" x="10" y="10" width="20" height="10" fill="none" stroke="none"/>
        <circle cx="45" cy="20" r="5" fill="#999"/>
        <rect id="both" x="70" y="0" width="10" height="10" fill="#000" stroke="#f00"/>
    </svg>"##;
    let out = reply(s.import_svg(args(json!({"data": svg}))));
    let pairs: Vec<(String, u64, String)> = out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["svg_id"].as_str().unwrap().into(), i["element"].as_u64().unwrap(), i["op"].as_str().unwrap().into()))
        .collect();
    let want = [("plate", 0, "cut"), ("cutter", 1, "no_output"), ("", 2, "no_output"), ("both", 3, "engrave"), ("both", 3, "cut")];
    assert_eq!(pairs, want.map(|(a, b, c)| (a.to_string(), b, c.to_string())));

    // The no-output cutter subtracted from the cut plate leaves a cut plate on its layer.
    let ids: Vec<u64> = out["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let layer = doc.lock().unwrap().item(ids[0]).unwrap().layer;
    let diff = reply(s.boolean(args(json!({"ids": [ids[0], ids[1]], "operation": "difference"}))));
    assert_eq!((diff["items"][0]["op"].as_str(), diff["items"][0]["layer"].as_u64(), diff["items"][0]["contours"].as_u64()), (Some("cut"), Some(layer), Some(2)));
}

#[test]
fn agent_changes_leave_the_selection_alone() {
    let (s, doc) = server();
    let ids = squares(&s, 3);
    assert!(doc.lock().unwrap().selection.is_empty(), "add_items selects nothing by default");
    reply(s.select(args(json!({"ids": [ids[0], ids[1]]}))));

    squares(&s, 2);
    reply(s.duplicate_items(args(json!({"ids": [ids[2]], "dx": 5, "dy": 0}))));
    reply(s.duplicate_items(args(json!({"dx": 5, "dy": 0}))));
    assert_eq!(doc.lock().unwrap().selection, [ids[0], ids[1]]);

    // Deleted items drop out of the selection.
    reply(s.delete_items(args(json!({"ids": [ids[1]]}))));
    assert_eq!(doc.lock().unwrap().selection, [ids[0]]);

    // Asking for it still selects.
    let new = reply(s.add_items(args(json!({"items": [{"shape": {"kind": "circle", "cx": 50, "cy": 50, "r": 5}}], "select": true}))));
    assert_eq!(json!(doc.lock().unwrap().selection), new["ids"]);
}
