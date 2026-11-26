//! The ribbon under the menu bar, as in AutoCAD: tabs (Home, Annotate, …), each a row of panels
//! with their title centred underneath. Main tools are large buttons (icon over label), the rest
//! small ones (icon and label, three to a column). When a tab does not fit the window, the small
//! buttons drop their labels; if it still does not fit, the ribbon scrolls sideways. The ⌃ at the
//! right folds it down to the tab names. The chosen tab and the fold are kept in the settings.

mod annotate;
mod arrange;
mod home;
mod laser;
mod parametric;
mod view;

use super::App;
use super::command::Cmd;
use super::icons::{help, paint_icon};
use super::shortcuts::{self, Action};
use eframe::egui::{self, Align, Color32, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
pub enum Tab {
    #[default]
    Home,
    Annotate,
    Parametric,
    Arrange,
    Laser,
    View,
}

impl Tab {
    const ALL: [Tab; 6] = [Tab::Home, Tab::Annotate, Tab::Parametric, Tab::Arrange, Tab::Laser, Tab::View];
}

#[derive(Default)]
pub(super) struct Ribbon {
    pub tab: Tab,
    pub collapsed: bool,
    /// Small buttons show only their icon, because the tab did not fit with labels.
    compact: bool,
    /// The width each tab needs with labels, measured when it was last drawn with them.
    full_width: [Option<f32>; 6],
    /// The open list of a split button's ways to draw.
    flyout: Option<Flyout>,
}

/// A split button's open list. `hold`: opened by pressing and holding, so letting go picks.
#[derive(Clone, Copy)]
struct Flyout {
    cmd: Cmd,
    hold: bool,
}

/// How long a press on a split button lasts before its list opens.
const HOLD: f64 = 0.35;

/// Height of the panels' button area, and of one small button (three fit in a column).
const BODY: f32 = 72.0;
const SMALL: f32 = 22.0;
const LARGE_ICON: f32 = 28.0;

/// What a button shows: a command's painted icon (`None` is Select) or a symbol from the font.
#[derive(Clone, Copy)]
pub(super) enum Icon {
    Cmd(Option<Cmd>),
    Glyph(&'static str),
    /// One way a tool draws (split buttons).
    Variant(Cmd, usize),
    /// A colour (layers).
    Swatch(Color32),
}

impl App {
    pub(in crate::gui) fn ribbon(&mut self, ui: &mut Ui, editing: bool) {
        ui.horizontal(|ui| {
            for (n, t) in Tab::ALL.into_iter().enumerate() {
                let name = ["Home", "Annotate", "Parametric", "Arrange", "Laser", "View"][n];
                if ui.selectable_label(self.ribbon.tab == t, RichText::new(name).size(14.0)).clicked() {
                    self.ribbon.tab = t;
                    self.ribbon.collapsed = false;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let tip = if self.ribbon.collapsed { "Show the ribbon" } else { "Fold the ribbon away (click a tab to show it again)" };
                if chevron(ui, self.ribbon.collapsed).on_hover_text(tip).clicked() {
                    self.ribbon.collapsed = !self.ribbon.collapsed;
                }
            });
        });
        if self.ribbon.collapsed {
            return;
        }
        let tab = self.ribbon.tab;
        let n = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
        self.ribbon.compact = self.ribbon.full_width[n].is_some_and(|w| w > ui.available_width());
        let width = egui::ScrollArea::horizontal()
            .id_salt("ribbon")
            // Dragging belongs to the buttons (split buttons open their list on press and drag).
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    match tab {
                        Tab::Home => ui.add_enabled_ui(editing, |ui| self.home(ui)).inner,
                        Tab::Annotate => ui.add_enabled_ui(editing, |ui| self.annotate(ui)).inner,
                        Tab::Parametric => ui.add_enabled_ui(editing, |ui| self.parametric(ui)).inner,
                        Tab::Arrange => ui.add_enabled_ui(editing, |ui| self.arrange(ui)).inner,
                        Tab::Laser => self.laser(ui),
                        Tab::View => self.view_tab(ui, editing),
                    }
                })
                .response
                .rect
                .width()
            })
            .inner;
        if !self.ribbon.compact {
            self.ribbon.full_width[n] = Some(width);
        }
    }

    /// A large button for a command (`None` is Select).
    fn large(&mut self, ui: &mut Ui, cmd: Option<Cmd>, label: &str) {
        let on = self.active_cmd() == cmd;
        if button(ui, true, false, on, Icon::Cmd(cmd), label).on_hover_text(cmd_tip(cmd, label)).clicked() {
            self.run(cmd);
        }
    }

    /// A large button with ▾ for a tool that draws in several ways: a click starts the way used last,
    /// ▾ lists the ways, and pressing and holding (or dragging off) opens the list to pick on release.
    fn split(&mut self, ui: &mut Ui, cmd: Cmd, label: &str) {
        let (names, chosen) = self.variants(cmd);
        let on = self.active_cmd() == Some(cmd);
        let open = self.ribbon.flyout.is_some_and(|f| f.cmd == cmd);
        let response = sensed_button(ui, Sense::click_and_drag(), true, false, on, Icon::Variant(cmd, chosen), &format!("{label}  "));
        let rect = response.rect;
        let c = if ui.is_enabled() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        let (x, y) = (rect.right() - 9.0, rect.top() + 12.0 + LARGE_ICON + 7.0);
        ui.painter().add(egui::Shape::convex_polygon(vec![pos2(x - 3.5, y - 2.0), pos2(x + 3.5, y - 2.0), pos2(x, y + 2.0)], c, Stroke::NONE));

        let down = response.is_pointer_button_down_on();
        if !open && down {
            let (held, moved) = ui.input(|i| {
                let held = i.pointer.press_start_time().map_or(0.0, |t| i.time - t);
                let moved = i.pointer.press_origin().zip(i.pointer.interact_pos()).map_or(0.0, |(a, b)| a.distance(b));
                (held, moved)
            });
            if held > HOLD || moved > 6.0 {
                self.ribbon.flyout = Some(Flyout { cmd, hold: true });
            } else {
                ui.ctx().request_repaint();
            }
        } else if !open && response.clicked() {
            if response.interact_pointer_pos().is_some_and(|p| p.x > rect.right() - 18.0 && p.y > rect.bottom() - 18.0) {
                self.ribbon.flyout = Some(Flyout { cmd, hold: false });
            } else {
                self.start_variant(cmd, chosen);
            }
        }
        let open = self.ribbon.flyout.is_some_and(|f| f.cmd == cmd);
        if open {
            self.flyout(ui.ctx(), rect, &names, chosen);
        } else if !down {
            // No tooltip while pressing or while the list is open: it would cover the list.
            response.on_hover_text(format!("{}\n\nNow: {}\nClick ▾ or press and hold for the other ways.", cmd_tip(Some(cmd), label), names[chosen]));
        }
    }

    /// The open list under a split button; picking a way starts the tool in it.
    fn flyout(&mut self, ctx: &egui::Context, under: Rect, names: &[&str], chosen: usize) {
        let Some(f) = self.ribbon.flyout else { return };
        let (pointer, pressed, released) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.any_pressed(), i.pointer.any_released()));
        let mut pick = None;
        let cmd = f.cmd;
        let area = egui::Area::new(egui::Id::NULL.with("ribbon-flyout")).order(egui::Order::Foreground).fixed_pos(under.left_bottom()).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                for (n, name) in names.iter().enumerate() {
                    let (rect, r) = ui.allocate_exact_size(vec2(190.0, 26.0), Sense::click());
                    let over = pointer.is_some_and(|p| rect.contains(p));
                    let v = ui.visuals();
                    let (bg, fg) = match (n == chosen, over) {
                        (_, true) => (v.widgets.hovered.bg_fill, v.strong_text_color()),
                        (true, false) => (v.selection.bg_fill.gamma_multiply(0.5), v.strong_text_color()),
                        _ => (Color32::TRANSPARENT, v.text_color()),
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(3), bg);
                    let icon = Rect::from_min_size(rect.min + vec2(5.0, 3.0), vec2(20.0, 20.0));
                    crate::gui::variant_icons::paint_variant(ui.painter(), icon, fg, cmd, n);
                    ui.painter().text(pos2(icon.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, *name, FontId::proportional(13.0), fg);
                    if r.clicked() || (f.hold && released && over) {
                        pick = Some(n);
                    }
                }
            });
        });
        let outside = pointer.is_none_or(|p| !area.response.rect.contains(p));
        if let Some(n) = pick {
            self.ribbon.flyout = None;
            self.start_variant(f.cmd, n);
        } else if (f.hold && released) || (pressed && outside) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ribbon.flyout = None;
        }
    }

    /// Small buttons for commands, three to a column.
    fn smalls(&mut self, ui: &mut Ui, cmds: &[(Option<Cmd>, &str)]) {
        for column in cmds.chunks(3) {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for &(cmd, label) in column {
                    let on = self.active_cmd() == cmd;
                    if button(ui, false, self.ribbon.compact, on, Icon::Cmd(cmd), label).on_hover_text(cmd_tip(cmd, label)).clicked() {
                        self.run(cmd);
                    }
                }
            });
        }
    }

    /// A small button that is not a command (a toggle or a window); returns whether it was clicked.
    fn small(&self, ui: &mut Ui, on: bool, icon: Icon, label: &str, tip: &str) -> bool {
        button(ui, false, self.ribbon.compact, on, icon, label).on_hover_text(format!("{label}\n{tip}")).clicked()
    }

    fn run(&mut self, cmd: Option<Cmd>) {
        match cmd {
            Some(c) => self.start(c),
            None => self.cancel(),
        }
    }
}

/// Tooltip: plain name, help and the shortcut.
fn cmd_tip(cmd: Option<Cmd>, label: &str) -> String {
    let name = cmd.map_or(label, |c| c.title());
    let key = match cmd {
        Some(c) => shortcuts::of(Action::Run(c)),
        None => Some("Esc"),
    };
    match key {
        Some(k) => format!("{name}\n{}\nShortcut: {k}", help(cmd)),
        None => format!("{name}\n{}", help(cmd)),
    }
}

/// A panel: its content in a row of `BODY` height, the title centred under it, a thin line after.
fn panel<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let r = ui.vertical(|ui| {
        let row = ui.horizontal_top(|ui| {
            ui.set_min_height(BODY);
            add(ui)
        });
        let w = row.response.rect.width();
        ui.allocate_ui_with_layout(vec2(w, 14.0), Layout::top_down(Align::Center), |ui| {
            ui.set_min_width(w);
            ui.label(RichText::new(title).small().weak());
        });
        row.inner
    });
    ui.add(egui::Separator::default().vertical().spacing(10.0));
    r.inner
}

/// A ribbon button. Large: icon over label. Small: icon and label side by side, or the icon alone.
pub(in crate::gui) fn button(ui: &mut Ui, large: bool, icon_only: bool, on: bool, icon: Icon, label: &str) -> egui::Response {
    sensed_button(ui, Sense::click(), large, icon_only, on, icon, label)
}

/// `button` with its own sense (split buttons also take drags).
fn sensed_button(ui: &mut Ui, sense: Sense, large: bool, icon_only: bool, on: bool, icon: Icon, label: &str) -> egui::Response {
    let font = FontId::proportional(if large { 12.0 } else { 13.0 });
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
    let size = if large {
        vec2((galley.size().x + 10.0).max(48.0), BODY)
    } else if icon_only {
        vec2(SMALL, SMALL)
    } else {
        vec2(SMALL + galley.size().x + 6.0, SMALL)
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let visuals = ui.visuals();
    let (bg, mut fg) = match (on, response.hovered()) {
        (true, _) => (crate::gui::theme::get().accent, crate::gui::theme::get().on_accent),
        (false, true) => (visuals.widgets.hovered.bg_fill, visuals.strong_text_color()),
        (false, false) => (Color32::TRANSPARENT, visuals.text_color()),
    };
    if !ui.is_enabled() {
        fg = fg.gamma_multiply(0.4);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), bg);
    let icon_rect = if large {
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 6.0 + LARGE_ICON / 2.0), vec2(LARGE_ICON, LARGE_ICON))
    } else {
        Rect::from_min_size(rect.min, vec2(SMALL, SMALL)).shrink(3.0)
    };
    match icon {
        Icon::Cmd(cmd) => paint_icon(painter, icon_rect, fg, cmd),
        Icon::Swatch(c) => {
            painter.rect_filled(icon_rect.shrink(4.0), CornerRadius::same(4), c);
            painter.rect_stroke(icon_rect.shrink(4.0), CornerRadius::same(4), Stroke::new(1.0, fg), egui::StrokeKind::Outside);
        }
        Icon::Variant(cmd, n) => crate::gui::variant_icons::paint_variant(painter, icon_rect, fg, cmd, n),
        Icon::Glyph(g) => _ = painter.text(icon_rect.center(), egui::Align2::CENTER_CENTER, g, FontId::proportional(icon_rect.height()), fg),
    }
    if large {
        painter.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 12.0 + LARGE_ICON), galley, fg);
    } else if !icon_only {
        painter.galley(pos2(rect.left() + SMALL + 2.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    }
    response
}

/// The fold button: a chevron pointing up (fold) or down (unfold).
fn chevron(ui: &mut Ui, down: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let c = if response.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
    let (y0, y1) = if down { (rect.center().y - 2.5, rect.center().y + 2.5) } else { (rect.center().y + 2.5, rect.center().y - 2.5) };
    let x = rect.center().x;
    ui.painter().line(vec![pos2(x - 5.0, y0), pos2(x, y1), pos2(x + 5.0, y0)], Stroke::new(1.6, c));
    response
}
