//! Layers, as in xTool Studio: every item sits on one layer, and the layer says what the laser does
//! with it (its op) and how hard (power, speed, passes; by default the material library's preset).
//! Hidden layers are neither drawn nor lasered or exported; locked ones cannot be picked or edited.

use super::{Document, Item, Op, State};
use crate::library::{Library, Operation, Setting};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// A layer's own laser numbers; a missing one comes from the material library's preset for the op.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Overrides {
    /// Power in percent (0–100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<f64>,
    /// Speed in mm/s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passes: Option<u32>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn check(&self) -> Result<(), String> {
        if self.power.is_some_and(|p| !(0.0..=100.0).contains(&p)) {
            return Err("power must be between 0 and 100 %".into());
        }
        if self.speed.is_some_and(|s| s <= 0.0) || self.passes == Some(0) {
            return Err("speed and passes must be larger than 0".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    /// What the laser does with the items on this layer.
    pub op: Op,
    /// Colour on the canvas as [r, g, b]; missing = the op's colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// Power, speed and passes for this layer; missing ones come from the material library.
    #[serde(default, skip_serializing_if = "Overrides::is_empty")]
    pub settings: Overrides,
    /// Distance between engraved lines in mm for this layer (engrave and images); missing = the material's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_interval: Option<f64>,
    /// The material's profile to use (e.g. "Photo"); missing or unknown = the default one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Hidden layers are not drawn, lasered or exported.
    #[serde(default = "yes")]
    pub visible: bool,
    /// Items on a locked layer cannot be selected or edited.
    #[serde(default)]
    pub locked: bool,
}

fn yes() -> bool {
    true
}

impl Layer {
    pub fn new(id: u64, name: &str, op: Op) -> Self {
        Self { id, name: name.into(), op, color: None, settings: Overrides::default(), line_interval: None, profile: None, visible: true, locked: false }
    }

    pub fn rgb(&self) -> [u8; 3] {
        self.color.unwrap_or(self.op.rgb())
    }

    /// The setting for `operation` on this layer: the library's preset with this layer's numbers on
    /// top (only for the layer's own operation: open shapes on an Engrave layer use the score preset).
    /// None when they do not give all of power, speed and passes.
    pub fn setting(&self, s: &State, lib: &Library, operation: Operation) -> Option<Setting> {
        let own = op_operation(self.op) == Some(operation);
        let profile = self.profile.as_deref().filter(|_| own);
        let preset = match (&s.machine, &s.material) {
            (Some(m), Some(mat)) => lib.profile(m, s.laser.as_deref(), mat, operation, profile).cloned(),
            _ => None,
        };
        let o = if own { self.settings } else { Overrides::default() };
        let mut out = preset.unwrap_or_else(|| Setting {
            power: f64::NAN,
            speed: f64::NAN,
            passes: 0,
            line_interval: None,
            bidirectional: None,
            ..Setting::new(s.machine.as_deref().unwrap_or_default(), s.laser.as_deref().unwrap_or_default(), operation)
        });
        out.power = o.power.unwrap_or(out.power);
        out.speed = o.speed.unwrap_or(out.speed);
        out.passes = o.passes.unwrap_or(out.passes);
        if own {
            out.line_interval = self.line_interval.or(out.line_interval);
        }
        (!out.power.is_nan() && !out.speed.is_nan() && out.passes > 0).then_some(out)
    }
}

/// The library operation of a layer op (no output has none).
pub fn op_operation(op: Op) -> Option<Operation> {
    match op {
        Op::Cut => Some(Operation::Cut),
        Op::Score => Some(Operation::Score),
        Op::Engrave => Some(Operation::Engrave),
        Op::NoOutput => None,
    }
}

/// The layers of a new document, one per op.
pub fn defaults() -> Vec<Layer> {
    Op::ALL.iter().zip(1..).map(|(op, id)| Layer::new(id, op.label(), *op)).collect()
}

impl State {
    pub fn layer(&self, id: u64) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: u64) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// The op of the item's layer.
    pub fn op(&self, item: &Item) -> Op {
        self.layer(item.layer).map_or(Op::Cut, |l| l.op)
    }

    /// What the laser really does: Engrave on an open shape (no area) is scored.
    pub fn effective_op(&self, item: &Item) -> Op {
        self.op(item).on(&item.shape)
    }

    /// Whether the laser engraves an area: Engrave on a shape that has one (closed path, text, image).
    pub fn engraves_area(&self, item: &Item) -> bool {
        self.op(item) == Op::Engrave && item.shape.has_area()
    }

    /// Whether the item's layer is shown (and so drawn, lasered and exported).
    pub fn visible(&self, item: &Item) -> bool {
        self.layer(item.layer).is_none_or(|l| l.visible)
    }

    /// Whether the item can be picked and edited: its layer is shown and not locked.
    pub fn editable(&self, item: &Item) -> bool {
        self.layer(item.layer).is_none_or(|l| l.visible && !l.locked)
    }

    /// The items on shown layers.
    pub fn shown(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| self.visible(i))
    }
}

impl Document {
    /// The layer new items go on: the chosen one, or the first when that one is gone.
    pub fn current_layer(&self) -> u64 {
        self.s.layer(self.current_layer).or(self.s.layers.first()).map_or(0, |l| l.id)
    }

    pub fn set_current_layer(&mut self, id: u64) {
        self.current_layer = id;
        self.rev += 1;
    }

    /// The first layer with `op`, made when there is none (callers checkpoint first).
    pub fn layer_for(&mut self, op: Op) -> u64 {
        match self.s.layers.iter().find(|l| l.op == op) {
            Some(l) => l.id,
            None => self.add_layer(op.label(), op),
        }
    }

    /// The layer for a new item: the current one; with an `op`, the current one when it has that op,
    /// else the first layer with it.
    pub fn new_item_layer(&mut self, op: Option<Op>) -> u64 {
        let current = self.current_layer();
        match op {
            Some(op) if self.s.layer(current).is_none_or(|l| l.op != op) => self.layer_for(op),
            _ => current,
        }
    }

    /// Add a layer at the end; a taken name gets a number ("Cut 2"). Returns its id.
    pub fn add_layer(&mut self, name: &str, op: Op) -> u64 {
        let taken = |n: &str| self.s.layers.iter().any(|l| l.name == n);
        let name = (1..).map(|k| if k == 1 { name.to_string() } else { format!("{name} {k}") }).find(|n| !taken(n)).unwrap();
        let id = self.new_id();
        self.s.layers.push(Layer::new(id, &name, op));
        self.touch();
        id
    }

    /// Delete a layer; its items move to `move_to`. Refused for the last layer, or when it has
    /// items and no layer to move them to.
    pub fn delete_layer(&mut self, id: u64, move_to: Option<u64>) -> Result<(), String> {
        self.s.layer(id).ok_or(format!("no layer with id {id}"))?;
        if self.s.layers.len() == 1 {
            return Err("a document needs at least one layer".into());
        }
        let used = self.s.items.iter().any(|i| i.layer == id);
        match move_to {
            Some(to) if to == id || self.s.layer(to).is_none() => return Err(format!("no other layer with id {to}")),
            Some(to) => self.s.items.iter_mut().filter(|i| i.layer == id).for_each(|i| i.layer = to),
            None if used => return Err("the layer has items: give a layer to move them to".into()),
            None => {}
        }
        self.s.layers.retain(|l| l.id != id);
        self.touch();
        Ok(())
    }

    /// Move a layer to position `index` in the list (clamped).
    pub fn move_layer(&mut self, id: u64, index: usize) -> Result<(), String> {
        let from = self.s.layers.iter().position(|l| l.id == id).ok_or(format!("no layer with id {id}"))?;
        let layer = self.s.layers.remove(from);
        self.s.layers.insert(index.min(self.s.layers.len()), layer);
        self.touch();
        Ok(())
    }

    /// Put items on a layer.
    pub fn move_to_layer(&mut self, ids: &[u64], layer: u64) -> Result<(), String> {
        self.s.layer(layer).ok_or(format!("no layer with id {layer}"))?;
        self.s.items.iter_mut().filter(|i| ids.contains(&i.id)).for_each(|i| i.layer = layer);
        self.touch();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::store;
    use crate::model::{Contour, Shape};

    #[test]
    fn items_take_the_op_of_their_layer() {
        let mut d = Document::new(100.0, 100.0);
        let closed = d.add("c", Op::Engrave, Shape::path(Contour::circle(10.0, 10.0, 5.0)));
        let open = d.add("o", Op::Engrave, Shape::path(Contour::line([0.0, 0.0], [5.0, 0.0])));
        let item = |d: &Document, id| d.item(id).unwrap().clone();
        assert!(d.s.engraves_area(&item(&d, closed)) && d.s.effective_op(&item(&d, open)) == Op::Score);
        assert_eq!(d.s.op(&item(&d, open)), Op::Engrave);
        // Changing the layer's op changes every item on it, and undo brings it back.
        let layer = item(&d, closed).layer;
        d.checkpoint();
        d.s.layer_mut(layer).unwrap().op = Op::Cut;
        assert_eq!(d.s.effective_op(&item(&d, closed)), Op::Cut);
        assert!(d.undo());
        assert_eq!(d.s.op(&item(&d, closed)), Op::Engrave);
    }

    #[test]
    fn adding_by_op_reuses_or_makes_a_layer() {
        let mut d = Document::new(100.0, 100.0);
        assert_eq!(d.s.layers.iter().map(|l| l.op).collect::<Vec<_>>(), Op::ALL);
        let a = d.add("a", Op::Cut, Shape::path(Contour::circle(10.0, 10.0, 5.0)));
        let cut = d.item(a).unwrap().layer;
        let score = d.layer_for(Op::Score);
        d.delete_layer(score, None).unwrap();
        let b = d.add("b", Op::Score, Shape::path(Contour::circle(10.0, 10.0, 5.0)));
        assert_ne!(d.item(b).unwrap().layer, cut);
        assert_eq!(d.s.layers.last().unwrap().name, "Score");
        assert_eq!(d.add_layer("Cut", Op::Cut), d.s.layers.last().unwrap().id);
        assert_eq!(d.s.layers.last().unwrap().name, "Cut 2");
    }

    #[test]
    fn deleting_a_layer_moves_or_refuses() {
        let mut d = Document::new(100.0, 100.0);
        let a = d.add("a", Op::Cut, Shape::path(Contour::circle(10.0, 10.0, 5.0)));
        let (cut, score) = (d.layer_for(Op::Cut), d.layer_for(Op::Score));
        assert!(d.delete_layer(cut, None).is_err(), "has items");
        d.checkpoint();
        d.delete_layer(cut, Some(score)).unwrap();
        assert_eq!(d.item(a).unwrap().layer, score);
        d.undo();
        assert_eq!((d.item(a).unwrap().layer, d.s.layers.len()), (cut, 4));
        d.move_layer(cut, 9).unwrap();
        assert_eq!(d.s.layers.last().unwrap().id, cut);
        for id in d.s.layers.iter().map(|l| l.id).collect::<Vec<_>>()[1..].iter() {
            d.delete_layer(*id, Some(d.s.layers[0].id)).unwrap();
        }
        assert!(d.delete_layer(d.s.layers[0].id, None).is_err(), "the last layer stays");
    }

    #[test]
    fn layer_settings_fall_back_to_the_material_preset() {
        let lib = store::merged(store::LibraryFile::default());
        let mut d = Document::new(100.0, 100.0);
        let cut = d.layer_for(Op::Cut);
        let mut layer = d.s.layer(cut).unwrap().clone();
        assert_eq!(layer.setting(&d.s, &lib, Operation::Cut), None, "no material chosen");
        (d.s.machine, d.s.laser, d.s.material) = (Some("xtool-m2".into()), Some("diode-10w".into()), Some("black-acrylic-3mm".into()));
        let preset = layer.setting(&d.s, &lib, Operation::Cut).unwrap();
        assert_eq!((preset.power, preset.speed, preset.passes), (100.0, 4.0, 2));
        layer.settings.passes = Some(3);
        let s = layer.setting(&d.s, &lib, Operation::Cut).unwrap();
        assert_eq!((s.power, s.speed, s.passes), (100.0, 4.0, 3));
        // A named profile of the material, with the layer's own numbers still on top.
        let mut lib = lib;
        lib.material_mut("black-acrylic-3mm").unwrap().put(Setting { name: "Fast".into(), speed: 9.0, ..preset.clone() });
        layer.profile = Some("Fast".into());
        assert_eq!(layer.setting(&d.s, &lib, Operation::Cut).unwrap().speed, 9.0);
        layer.profile = Some("Gone".into());
        assert_eq!(layer.setting(&d.s, &lib, Operation::Cut).unwrap().speed, 4.0, "an unknown profile uses the default");
        // No preset for engrave: only a full override gives a setting.
        layer.op = Op::Engrave;
        assert_eq!(layer.setting(&d.s, &lib, Operation::Engrave), None);
        layer.settings = Overrides { power: Some(20.0), speed: Some(100.0), passes: Some(1) };
        assert_eq!(layer.setting(&d.s, &lib, Operation::Engrave).unwrap().hint(), "20 % · 100 mm/s · 1×");
        assert_eq!(layer.setting(&d.s, &lib, Operation::Score), None, "overrides are for the layer's own op");
    }
}
