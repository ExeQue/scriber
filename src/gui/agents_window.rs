//! "Connect AI agents": lists the agents found on this computer and adds/removes Scriber's MCP entry.

use super::App;
use crate::agents::{self, Status, Target};
use eframe::egui::{self, Color32, RichText};

impl App {
    /// Open the window and look for agents in the background (WSL lookups can take seconds).
    pub(super) fn open_agents(&mut self) {
        self.show_agents = true;
        let list = self.agents.clone();
        *list.lock().unwrap() = None;
        std::thread::spawn(move || {
            let found = agents::detect();
            *list.lock().unwrap() = Some(found);
        });
    }

    pub(super) fn agents_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_agents;
        let mut refresh = false;
        let list = self.agents.lock().unwrap().clone();
        egui::Window::new("Connect AI agents").open(&mut open).default_width(580.0).show(ctx, |ui| {
            ui.label("Scriber found these AI agents on this computer. Click Connect, then restart the agent: it can then draw in this window.");
            ui.add_space(6.0);
            match &list {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking for agents…");
                    });
                }
                Some(list) if list.is_empty() => _ = ui.label("No supported agents found."),
                Some(list) => {
                    egui::Grid::new("agents").num_columns(4).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                        for t in list {
                            refresh |= self.agent_row(ui, t);
                            ui.end_row();
                        }
                    });
                }
            }
            ui.add_space(6.0);
            if !self.message.is_empty() {
                ui.label(RichText::new(&self.message).small());
            }
            ui.horizontal(|ui| {
                refresh |= ui.button("Search again").clicked();
                ui.label(RichText::new("Command line: scriber agents").small().weak());
            });
        });
        self.show_agents = open;
        if refresh {
            self.open_agents();
        }
        if list.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// One agent: name, where it lives, status, buttons. Returns true when something changed.
    fn agent_row(&mut self, ui: &mut egui::Ui, t: &Target) -> bool {
        ui.label(RichText::new(&t.name).strong()).on_hover_text(&t.config);
        ui.label(&t.host);
        let (text, color) = match &t.status {
            Status::Installed => ("Connected".to_string(), Color32::from_rgb(40, 160, 70)),
            Status::Outdated => ("Points to another Scriber".to_string(), Color32::from_rgb(210, 140, 0)),
            Status::NotInstalled => ("Not connected".to_string(), ui.visuals().weak_text_color()),
            Status::Error(e) => (format!("{}: {e}", "Error"), Color32::from_rgb(200, 50, 50)),
        };
        ui.label(RichText::new(text).color(color));
        let mut changed = false;
        ui.horizontal(|ui| {
            if t.status != Status::Installed && ui.button("Connect").clicked() {
                self.message = match agents::install(t) {
                    Ok(()) => format!("{} {}. {}", "Connected", t.name, "Restart the agent to use Scriber."),
                    Err(e) => format!("{}: {e}", t.name),
                };
                changed = true;
            }
            if matches!(t.status, Status::Installed | Status::Outdated) && ui.button("Remove").clicked() {
                self.message = match agents::remove(t) {
                    Ok(()) => format!("{} {}", "Removed from", t.name),
                    Err(e) => format!("{}: {e}", t.name),
                };
                changed = true;
            }
        });
        changed
    }
}
