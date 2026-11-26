//! Parametric: constraints that bind shapes to each other, and locked sizes (parameters).

use super::{App, Icon, panel};
use crate::gui::command::Cmd;
use crate::model::Kind;
use eframe::egui::Ui;

const GEOMETRIC: [Kind; 12] = [
    Kind::Coincident,
    Kind::Collinear,
    Kind::Concentric,
    Kind::Fix,
    Kind::Parallel,
    Kind::Perpendicular,
    Kind::Horizontal,
    Kind::Vertical,
    Kind::Tangent,
    Kind::Smooth,
    Kind::Symmetric,
    Kind::Equal,
];

const SIZES: [Cmd; 7] = [
    Cmd::DcLinear,
    Cmd::Constrain(Kind::DimHorizontal),
    Cmd::Constrain(Kind::DimVertical),
    Cmd::Constrain(Kind::DimAligned),
    Cmd::Constrain(Kind::DimAngular),
    Cmd::Constrain(Kind::DimRadial),
    Cmd::Constrain(Kind::DimDiameter),
];

impl App {
    pub(super) fn parametric(&mut self, ui: &mut Ui) {
        panel(ui, "Geometric", |ui| {
            self.large(ui, Some(Cmd::AutoConstrain), "Find");
            let cmds: Vec<_> = GEOMETRIC.iter().map(|&k| (Some(Cmd::Constrain(k)), Cmd::Constrain(k).title())).collect();
            self.smalls(ui, &cmds);
        });
        panel(ui, "Lock sizes", |ui| {
            // >>> The large "Dimension" button (smart DIM) goes here; until then, Lock distance stands in.
            self.large(ui, Some(Cmd::DcLinear), "Lock distance");
            // <<<
            ui.vertical(|ui| {
                ui.menu_button("More ▾", |ui| {
                    for c in SIZES {
                        let on = self.active_cmd() == Some(c);
                        if super::button(ui, false, false, on, Icon::Cmd(Some(c)), c.title()).on_hover_text(super::cmd_tip(Some(c), c.title())).clicked() {
                            self.start(c);
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Every way to lock a size");
            });
        });
        panel(ui, "Manage", |ui| {
            self.large(ui, Some(Cmd::Parameters), "Parameters");
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                self.smalls(ui, &[(Some(Cmd::DelConstraint), "Remove constraints")]);
                let tip = "Show or hide the constraint icons: for the selected shapes, or all when nothing is selected";
                if self.small(ui, self.bars, Icon::Cmd(Some(Cmd::ConstraintBar)), "Show constraints", tip) {
                    self.start(Cmd::ConstraintBar);
                }
            });
        });
    }
}
