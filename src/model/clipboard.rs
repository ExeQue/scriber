//! Copy and paste: the selection as a JSON payload (`{"scriber-clipboard": 1, …}`) that goes on the
//! system clipboard as text, so it can be pasted into another Scriber window.

use super::constraints::{self, expr};
use super::geom::{self, BBox, Transform};
use super::{Constraint, Document, Guide, Img, Item, Ref, Shape, State};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// What Copy puts on the clipboard. Ids are the source document's; paste gives new ones.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    #[serde(rename = "scriber-clipboard")]
    pub version: u32,
    pub items: Vec<Item>,
    /// Only constraints between copied items.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub groups: Vec<Vec<u64>>,
    #[serde(default)]
    pub images: Vec<ClipImage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipImage {
    pub id: u64,
    pub w: u32,
    pub h: u32,
    /// PNG, base64.
    pub png: String,
}

impl Clip {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A payload Copy wrote; None for any other text.
    pub fn from_json(text: &str) -> Option<Clip> {
        let text = text.trim_start();
        if !text.starts_with('{') || !text.contains("\"scriber-clipboard\"") {
            return None;
        }
        serde_json::from_str(text).ok()
    }

    pub fn bbox(&self) -> Option<BBox> {
        geom::union(&self.items.iter().map(|i| i.shape.metrics().bbox).collect::<Vec<_>>())
    }
}

fn set_item(r: &mut Ref, new: u64) {
    match r {
        Ref::Point { item, .. } | Ref::Segment { item, .. } | Ref::Centre { item, .. } | Ref::Item { item } => *item = new,
    }
}

impl Document {
    /// The items `ids` with their constraints, groups and images; None when none exist.
    pub fn copy(&self, ids: &[u64]) -> Option<Clip> {
        let items: Vec<Item> = self.s.items.iter().filter(|i| ids.contains(&i.id)).cloned().collect();
        if items.is_empty() {
            return None;
        }
        let has = |id: u64| items.iter().any(|i| i.id == id);
        // Parameters that use one left behind keep their value instead.
        let mut only = State { constraints: self.s.constraints.iter().filter(|c| c.refs.iter().all(|r| has(r.item()))).cloned().collect(), ..State::default() };
        constraints::freeze_orphans(&mut only, &constraints::parameter_values(&self.s).unwrap_or_default());
        let groups = self.s.groups.iter().map(|g| g.iter().copied().filter(|id| has(*id)).collect::<Vec<_>>()).filter(|g| g.len() >= 2).collect();
        let images = items
            .iter()
            .filter_map(|i| if let Shape::Image { image, .. } = i.shape { Some(image) } else { None })
            .filter_map(|id| self.s.images.get(&id).map(|img| ClipImage { id, w: img.w, h: img.h, png: base64::engine::general_purpose::STANDARD.encode(&img.png) }))
            .collect();
        Some(Clip { version: 1, items, constraints: only.constraints, groups, images })
    }

    /// Add copies of the clip's items moved by (dx, dy), with new ids; they become the selection.
    /// Items keep their layer when it exists and is not locked, else go on the current layer.
    pub fn paste(&mut self, clip: &Clip, dx: f64, dy: f64) -> Vec<u64> {
        let mut images = HashMap::new();
        for ci in &clip.images {
            let Ok(png) = base64::engine::general_purpose::STANDARD.decode(&ci.png) else { continue };
            let id = self.new_id();
            self.s.images.insert(id, Arc::new(Img { png, w: ci.w, h: ci.h }));
            images.insert(ci.id, id);
        }
        let map: HashMap<u64, u64> = clip.items.iter().map(|i| (i.id, self.new_id())).collect();
        let current = self.current_layer();
        let t = Transform::translate(dx, dy);
        for item in &clip.items {
            let mut item = item.clone();
            item.id = map[&item.id];
            if self.s.layer(item.layer).is_none_or(|l| l.locked) {
                item.layer = current;
            }
            item.shape = item.shape.transformed(&t, [0.0, 0.0]);
            match &mut item.shape {
                Shape::Image { image, .. } => *image = images.get(image).copied().unwrap_or(*image),
                // Guides follow their copied shapes, and stop following ones left behind.
                Shape::Guide(Guide::Centerline { of, .. }) => *of = of.iter().map(|id| map.get(id).copied()).collect::<Option<Vec<_>>>().unwrap_or_default(),
                Shape::Guide(Guide::Centermark { of, .. }) => *of = of.and_then(|id| map.get(&id).copied()),
                _ => {}
            }
            self.s.items.push(item);
        }
        // Parameter names must stay unique: taken ones get the next free name, in every pasted expression too.
        let mut cons = clip.constraints.clone();
        let mut used: Vec<String> = self.s.constraints.iter().chain(&cons).filter(|c| c.kind.is_dimension()).map(|c| c.name.clone()).collect();
        for n in 0..cons.len() {
            if !cons[n].kind.is_dimension() || self.parameter(&cons[n].name).is_none() {
                continue;
            }
            let old = cons[n].name.clone();
            let new = (1..).map(|k| format!("{}{k}", cons[n].kind.prefix())).find(|m| !used.contains(m)).unwrap();
            used.push(new.clone());
            for c in &mut cons {
                c.expr = expr::rename(&c.expr, &old, &new);
            }
            cons[n].name = new;
        }
        for mut c in cons {
            if c.refs.iter().any(|r| !map.contains_key(&r.item())) {
                continue;
            }
            c.id = self.new_id();
            for r in &mut c.refs {
                set_item(r, map[&r.item()]);
            }
            self.s.constraints.push(c);
        }
        for g in &clip.groups {
            self.s.groups.push(g.iter().filter_map(|id| map.get(id).copied()).collect());
        }
        let ids: Vec<u64> = clip.items.iter().map(|i| map[&i.id]).collect();
        self.selection = ids.clone();
        self.touch();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Kind, Op};

    fn line(d: &mut Document, x: f64) -> u64 {
        d.add("l", Op::Cut, Shape::path(crate::model::Contour::line([x, 0.0], [x + 10.0, 0.0])))
    }

    #[test]
    fn copy_paste_remaps_constraints_and_falls_back_to_current_layer() {
        let mut d = Document::new(100.0, 100.0);
        let (a, b, c) = (line(&mut d, 0.0), line(&mut d, 20.0), line(&mut d, 40.0));
        d.add_constraint(Kind::Parallel, vec![Ref::Segment { item: a, contour: 0, segment: 0 }, Ref::Segment { item: b, contour: 0, segment: 0 }]).unwrap();
        d.add_constraint(Kind::Parallel, vec![Ref::Segment { item: b, contour: 0, segment: 0 }, Ref::Segment { item: c, contour: 0, segment: 0 }]).unwrap();
        d.add_dimension(Kind::DimAligned, vec![Ref::Point { item: a, contour: 0, vertex: 0 }, Ref::Point { item: a, contour: 0, vertex: 1 }], None, None, 5.0).unwrap();
        d.group(&[a, b]).unwrap();
        let clip = Clip::from_json(&d.copy(&[a, b]).unwrap().to_json()).unwrap();
        assert_eq!(clip.constraints.len(), 2, "b–c is dropped");

        // Pasting into a document without the source layer puts the copies on its current layer.
        let mut e = Document::new(100.0, 100.0);
        e.s.layers.retain(|l| l.op != Op::Cut);
        let ids = e.paste(&clip, 10.0, 10.0);
        assert_eq!(ids.len(), 2);
        assert_eq!(e.selection, ids);
        assert!(e.s.items.iter().all(|i| i.layer == e.current_layer()));
        assert_eq!(e.s.items[0].shape.contours()[0].vertices[0].x, 10.0);
        assert_eq!(e.s.groups, vec![ids.clone()]);
        assert!(e.s.constraints.iter().all(|c| c.refs.iter().all(|r| ids.contains(&r.item()))));

        // Pasting back into the source keeps the layer and renames the taken parameter.
        let before = d.s.constraints.len();
        let ids = d.paste(&clip, 0.0, 0.0);
        assert_eq!(d.s.constraints.len(), before + 2);
        assert!(d.parameter("d1").is_some() && d.parameter("d2").is_some_and(|p| ids.contains(&p.refs[0].item())));
        assert_eq!(d.item(ids[0]).unwrap().layer, d.item(a).unwrap().layer);
        assert!(d.settle(false).is_ok());
    }

    #[test]
    fn payload_round_trip_keeps_images() {
        let mut d = Document::new(100.0, 100.0);
        let img = crate::export::image::to_grey(&image_png()).unwrap();
        let id = d.place_image(img, [5.0, 5.0], Some(20.0), "pic");
        let json = d.copy(&[id]).unwrap().to_json();
        assert!(json.starts_with("{\"scriber-clipboard\":1"));
        assert!(Clip::from_json("hello").is_none());
        let clip = Clip::from_json(&json).unwrap();
        let ids = d.paste(&clip, 10.0, 0.0);
        let Shape::Image { image, x, .. } = d.item(ids[0]).unwrap().shape else { panic!() };
        assert_eq!(x, 15.0);
        assert_eq!(d.s.images[&image].png, d.s.images.values().next().unwrap().png);
        assert_eq!(d.s.images.len(), 2);
    }

    fn image_png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::GrayImage::from_pixel(3, 2, image::Luma([128])).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }
}
