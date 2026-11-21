//! The phone layout: the document on its own, wrapped to the screen's width (Web Layout in Full
//! Screen View), a bar of large buttons along the top (File, Undo, Redo, Save, All commands) and a
//! floating toolbar along the bottom (Insert, Bold, Italic, Underline, Highlight, Font Color,
//! More), as Word's phone app does. Screens whose shorter side is at least [`PHONE_WIDTH`]
//! (tablets, a DeX window) keep the desktop interface; "⋯" on the phone shows it too, until
//! "Phone view". A phone keeps the phone layout in landscape and with the keyboard open.
//!
//! Everything here goes through `WordApp::run` (the commands the ribbon uses), so it needs no
//! change to WordCraft itself. Kept free of Android types, so it builds (and is checked) anywhere.
//!
//! The page keeps the keyboard while the bars are tapped: egui normally takes focus away from a
//! widget when the pointer clicks elsewhere, which on a phone closes the on-screen keyboard and
//! drops a pending format (Bold, then type). The phone layout turns that off; text fields still
//! take focus when tapped, and the page takes it back when tapped.
//!
//! For the emulator test (android/test), every button's centre is logged in pixels whenever the
//! layout changes (`autotest button <name> <x> <y>`), so a script can tap it, and so are the
//! document's text and the formatting at the caret (`autotest doc ...`), so the test can check
//! typing and Bold without reading pixels.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};
use wordcraft_ui_egui::{WordApp, icons};

/// Screens whose shorter side is narrower than this (in points) get the phone layout.
pub const PHONE_WIDTH: f32 = 600.0;
/// The top bar's height and a button's side: Android's 48 dp touch target.
const BAR: f32 = 52.0;
const BUTTON: f32 = 48.0;
/// The floating toolbar: its height and its margin from the screen's edges.
const TOOLBAR: f32 = 60.0;
const MARGIN: f32 = 10.0;
/// Full Screen View's own slim bar (`chrome::SCREEN_BAR_HEIGHT`), hidden under the top bar.
const SCREEN_BAR: f32 = wordcraft_ui_egui::chrome::SCREEN_BAR_HEIGHT;

/// Highlight colours offered on the phone (OOXML names) and their swatches.
const HIGHLIGHTS: [(&str, Color32); 5] = [
    ("yellow", Color32::from_rgb(255, 255, 0)),
    ("green", Color32::from_rgb(0, 255, 0)),
    ("cyan", Color32::from_rgb(0, 255, 255)),
    ("magenta", Color32::from_rgb(255, 0, 255)),
    ("none", Color32::TRANSPARENT),
];
/// Font colours offered on the phone (hex, as `format.color` takes them; "auto" is Automatic).
const COLORS: [(&str, Color32); 7] = [
    ("auto", Color32::from_rgb(0, 0, 0)),
    ("C00000", Color32::from_rgb(0xC0, 0, 0)),
    ("FF0000", Color32::from_rgb(0xFF, 0, 0)),
    ("FFC000", Color32::from_rgb(0xFF, 0xC0, 0)),
    ("00B050", Color32::from_rgb(0, 0xB0, 0x50)),
    ("0070C0", Color32::from_rgb(0, 0x70, 0xC0)),
    ("7030A0", Color32::from_rgb(0x70, 0x30, 0xA0)),
];

/// Below this height (in points) the phone editor puts all its buttons in one row.
const COMPACT_HEIGHT: f32 = 300.0;

/// A button of the phone editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    File,
    Undo,
    Redo,
    Save,
    /// "⋯" in the top bar.
    AllCommands,
    Insert,
    Bold,
    Italic,
    Underline,
    Highlight,
    Color,
    /// "⋯" in the toolbar.
    More,
}

/// The floating toolbar's buttons.
const TOOLBAR_TOOLS: [Tool; 7] = [Tool::Insert, Tool::Bold, Tool::Italic, Tool::Underline, Tool::Highlight, Tool::Color, Tool::More];
/// The single row of the compact layout: the top bar's buttons and the toolbar's.
const COMPACT_TOOLS: [Tool; 11] = [
    Tool::File,
    Tool::Undo,
    Tool::Redo,
    Tool::Save,
    Tool::Insert,
    Tool::Bold,
    Tool::Italic,
    Tool::Underline,
    Tool::Highlight,
    Tool::Color,
    Tool::More,
];

impl Tool {
    /// The name the emulator test taps it by, and its icon (File is a text button).
    fn name(self) -> &'static str {
        match self {
            Tool::File => "file",
            Tool::Undo => "undo",
            Tool::Redo => "redo",
            Tool::Save => "save",
            Tool::AllCommands => "more",
            Tool::Insert => "addShape",
            Tool::Bold => "bold",
            Tool::Italic => "italic",
            Tool::Underline => "underline",
            Tool::Highlight => "highlight",
            Tool::Color => "fontcolor",
            Tool::More => "toolbarMore",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tool::AllCommands | Tool::More => "more",
            t => t.name(),
        }
    }

    /// The accessible name (the visible label for File).
    fn label(self) -> &'static str {
        match self {
            Tool::File => "Datei",
            Tool::Undo => "Rückgängig",
            Tool::Redo => "Wiederholen",
            Tool::Save => "Speichern",
            Tool::AllCommands => "Alle Befehle",
            Tool::Insert => "Einfügen",
            Tool::Bold => "Fett",
            Tool::Italic => "Kursiv",
            Tool::Underline => "Unterstrichen",
            Tool::Highlight => "Hervorheben",
            Tool::Color => "Schriftfarbe",
            Tool::More => "Weitere Befehle",
        }
    }
}

/// What the screen shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// The phone editor: top bar, the document, the floating toolbar.
    Phone,
    /// WordCraft's own interface: tablets, File (Backstage), or "⋯ › All commands" on a phone.
    Desktop,
}

/// A menu opened from the floating toolbar, drawn just above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Menu {
    Insert,
    Highlight,
    Color,
}

#[derive(Default)]
pub struct Mobile {
    /// Log button positions and the document state for the emulator test (android/test).
    pub autotest: bool,
    /// "⋯" asked for the full interface on a phone; "Phone view" ends it.
    all_commands: bool,
    menu: Option<Menu>,
    shown: Option<Shown>,
    /// The button positions as last logged for the emulator test.
    logged: String,
    /// Button centres (name, point) gathered while drawing, logged when they change.
    buttons: Vec<(&'static str, egui::Pos2)>,
    /// The document state as last logged for the emulator test.
    logged_doc: String,
}

impl Mobile {
    /// Lay out the app in `area` (the part of the window the system bars leave free).
    /// The phone layout; `autotest` logs what the emulator test checks (button positions, text).
    pub fn new(autotest: bool) -> Self {
        Self { autotest, ..Default::default() }
    }

    /// `smallest_width`: the screen's shorter side in points, whichever way it is turned (the
    /// area's width when unknown).
    pub fn ui(&mut self, app: &mut WordApp, ui: &mut Ui, area: Rect, smallest_width: Option<f32>) {
        let phone = smallest_width.unwrap_or(area.width()) < PHONE_WIDTH;
        let shown = if phone && !self.all_commands && !app.ui.backstage { Shown::Phone } else { Shown::Desktop };
        // Leaving the start page without choosing a document leaves none open, and the phone
        // editor then has nothing to show (WordCraft's empty window with the ribbon): start a
        // blank document instead, as the start page's own first choice does.
        if shown == Shown::Phone
            && !app.has_document()
            && let Err(e) = app.run("file.new", json!({}))
        {
            log::warn!("no document and none could be created: {e}");
        }
        self.switch(app, ui.ctx(), shown, phone);
        self.buttons.clear();
        match shown {
            Shown::Desktop => {
                ui.scope_builder(UiBuilder::new().max_rect(area), |ui| app.ui(ui));
                if phone && self.all_commands && !app.ui.backstage {
                    self.phone_view_button(ui, area);
                }
            }
            Shown::Phone => self.phone_ui(app, ui, area),
        }
        if self.autotest {
            self.log_buttons(ui.ctx().pixels_per_point());
            self.log_doc(app);
        }
    }

    /// Log the body text and the formatting at the caret when they change (for android/test).
    fn log_doc(&mut self, app: &mut WordApp) {
        let state: Value = app.session.run("format.state", &json!({})).unwrap_or_default();
        let on = |k: &str| state.get(k).and_then(Value::as_bool).unwrap_or(false);
        let line = format!(
            "autotest doc bold={} italic={} underline={} fullScreenView={} text={:?}",
            on("bold"),
            on("italic"),
            on("underline"),
            app.session.view.full_screen_view,
            app.session.doc.plain_text(wordcraft_doc::StoryRef::Body),
        );
        if line != self.logged_doc {
            log::info!("{line}");
            self.logged_doc = line;
        }
    }

    /// Set the views each layout needs when the screen changes between them.
    fn switch(&mut self, app: &mut WordApp, ctx: &egui::Context, shown: Shown, phone: bool) {
        if self.shown == Some(shown) {
            return;
        }
        // Taps on the bars must not take the keyboard from the page (see the module docs).
        let surrender = if shown == Shown::Phone { egui::SurrenderFocusOn::Never } else { egui::SurrenderFocusOn::default() };
        ctx.options_mut(|o| o.input_options.surrender_focus_on = surrender);
        let first = self.shown.is_none();
        self.shown = Some(shown);
        self.menu = None;
        match shown {
            // The text wrapped to the screen, without the window's chrome.
            Shown::Phone => {
                let _ = app.run("view.webLayout", json!({}));
                let _ = app.run("view.fullScreenView", json!({"value": true}));
            }
            // Back to the usual window; on a phone, also back to the pages (Print Layout).
            Shown::Desktop if !first => {
                let _ = app.run("view.fullScreenView", json!({"value": false}));
                if phone && !app.ui.backstage {
                    let _ = app.run("view.printLayout", json!({}));
                }
            }
            Shown::Desktop => {}
        }
    }

    fn phone_ui(&mut self, app: &mut WordApp, ui: &mut Ui, area: Rect) {
        // Kept every frame, not only on entry: the File page, Escape or a ribbon command may
        // leave Full Screen View or Web Layout, and the phone editor needs both.
        if format!("{:?}", app.session.view.mode) != "Web" {
            let _ = app.run("view.webLayout", json!({}));
        }
        if !app.session.view.full_screen_view {
            let _ = app.run("view.fullScreenView", json!({"value": true}));
        }
        // Too little height for two bars (landscape with the keyboard open): one row along the
        // bottom with every button, and the document above it.
        let compact = area.height() < COMPACT_HEIGHT;
        let (top, toolbar) = if compact {
            let toolbar = Rect::from_min_max(pos2(area.min.x + 4.0, area.max.y - 4.0 - BUTTON), pos2(area.max.x - 4.0, area.max.y - 4.0));
            (Rect::from_min_max(area.min, pos2(area.max.x, area.min.y)), toolbar)
        } else {
            let top = Rect::from_min_max(area.min, pos2(area.max.x, area.min.y + BAR));
            let toolbar =
                Rect::from_min_max(pos2(area.min.x + MARGIN, area.max.y - MARGIN - TOOLBAR), pos2(area.max.x - MARGIN, area.max.y - MARGIN));
            (top, toolbar)
        };
        // The document: from under the top bar (Full Screen View's own bar is cut off above it)
        // to above the toolbar, so the caret is never behind either.
        let doc = Rect::from_min_max(pos2(area.min.x, top.max.y - SCREEN_BAR), pos2(area.max.x, toolbar.min.y - 4.0));
        let shown = Rect::from_min_max(pos2(area.min.x, top.max.y), doc.max);
        let (bg, fg) = (ui.visuals().panel_fill, ui.visuals().text_color());
        ui.painter().rect_filled(Rect::from_min_max(pos2(area.min.x, doc.max.y), area.max), 0.0, ui.visuals().extreme_bg_color);
        ui.scope_builder(UiBuilder::new().max_rect(doc), |ui| {
            ui.set_clip_rect(shown);
            app.ui(ui)
        });

        let state: Value = app.session.run("format.state", &json!({})).unwrap_or_default();
        let on = |k: &str| state.get(k).and_then(Value::as_bool).unwrap_or(false);
        let active = |t: Tool, menu: Option<Menu>| match t {
            Tool::Bold => on("bold"),
            Tool::Italic => on("italic"),
            Tool::Underline => on("underline"),
            Tool::Insert => menu == Some(Menu::Insert),
            Tool::Highlight => menu == Some(Menu::Highlight),
            Tool::Color => menu == Some(Menu::Color),
            _ => false,
        };
        let accent = ui.visuals().selection.bg_fill;
        let line = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
        let mut pressed = None;

        // The bars above the document, in their own layer so they get the touches first.
        if !compact {
            egui::Area::new(egui::Id::unique("phone_top_bar")).order(egui::Order::Foreground).fixed_pos(top.min).show(ui.ctx(), |ui| {
                ui.set_clip_rect(top);
                ui.painter().rect_filled(top, 0.0, bg);
                ui.painter().line_segment([top.left_bottom(), top.right_bottom()], line);
                let y = top.center().y;
                if self.tool_button(ui, Rect::from_center_size(pos2(top.min.x + 44.0, y), vec2(80.0, BUTTON)), Tool::File, false, fg, accent) {
                    pressed = Some(Tool::File);
                }
                let mut x = top.max.x - 4.0 - BUTTON / 2.0;
                for t in [Tool::AllCommands, Tool::Save, Tool::Redo, Tool::Undo] {
                    if self.tool_button(ui, Rect::from_center_size(pos2(x, y), Vec2::splat(BUTTON)), t, false, fg, accent) {
                        pressed = Some(t);
                    }
                    x -= BUTTON;
                }
            });
        }
        let tools: &[Tool] = if compact { &COMPACT_TOOLS } else { &TOOLBAR_TOOLS };
        let menu = self.menu;
        egui::Area::new(egui::Id::unique("phone_toolbar")).order(egui::Order::Foreground).fixed_pos(toolbar.min).show(ui.ctx(), |ui| {
            let radius = (toolbar.height() / 2.0) as u8;
            ui.painter().rect(toolbar, CornerRadius::same(radius), bg, line, egui::StrokeKind::Inside);
            let step = toolbar.width() / tools.len() as f32;
            for (i, &t) in tools.iter().enumerate() {
                let centre = pos2(toolbar.min.x + step * (i as f32 + 0.5), toolbar.center().y);
                let size = if t == Tool::File { vec2(step.min(80.0), BUTTON) } else { Vec2::splat(BUTTON.min(step)) };
                if self.tool_button(ui, Rect::from_center_size(centre, size), t, active(t, menu), fg, accent) {
                    pressed = Some(t);
                }
            }
        });
        if let Some(t) = pressed {
            self.run_tool(app, t);
        }

        if let Some(menu) = self.menu {
            let above = Rect::from_min_max(pos2(toolbar.min.x, toolbar.min.y - MARGIN - BUTTON - 16.0), pos2(toolbar.max.x, toolbar.min.y - MARGIN));
            let picked = egui::Area::new(egui::Id::unique("phone_menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(above.min)
                .show(ui.ctx(), |ui| {
                    ui.painter().rect(
                        above,
                        CornerRadius::same(16),
                        bg,
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                        egui::StrokeKind::Inside,
                    );
                    self.menu_ui(app, ui, menu, above, fg, accent)
                })
                .inner;
            if picked {
                self.menu = None;
            }
        }
    }

    /// Draw one button and note its centre for the test; true when it was tapped.
    fn tool_button(&mut self, ui: &mut Ui, r: Rect, t: Tool, active: bool, fg: Color32, accent: Color32) -> bool {
        self.buttons.push((t.name(), r.center()));
        let response = if t == Tool::File { text_button(ui, r, t.label(), fg) } else { icon_button(ui, r, t.icon(), t.label(), active, fg, accent) };
        response.clicked()
    }

    fn run_tool(&mut self, app: &mut WordApp, t: Tool) {
        let command = match t {
            Tool::File => {
                let _ = app.run("ui.backstage", json!({"value": true, "page": "home"}));
                return;
            }
            Tool::AllCommands | Tool::More => {
                self.all_commands = true;
                return;
            }
            Tool::Insert => return self.toggle(Menu::Insert),
            Tool::Highlight => return self.toggle(Menu::Highlight),
            Tool::Color => return self.toggle(Menu::Color),
            Tool::Undo => "edit.undo",
            Tool::Redo => "edit.redo",
            Tool::Save => "file.save",
            Tool::Bold => "format.bold",
            Tool::Italic => "format.italic",
            Tool::Underline => "format.underline",
        };
        let _ = app.run(command, json!({}));
    }

    /// Log every button's centre in screen pixels when the layout changes (for android/test).
    fn log_buttons(&mut self, pixels_per_point: f32) {
        let mut line = String::new();
        for (name, p) in &self.buttons {
            line.push_str(&format!("autotest button {name} {:.0} {:.0}\n", p.x * pixels_per_point, p.y * pixels_per_point));
        }
        if line != self.logged {
            for l in line.lines() {
                log::info!("{l}");
            }
            self.logged = line;
        }
    }

    fn toggle(&mut self, menu: Menu) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
    }

    /// The row of choices for `menu`; true when one was picked.
    fn menu_ui(&mut self, app: &mut WordApp, ui: &mut Ui, menu: Menu, r: Rect, fg: Color32, accent: Color32) -> bool {
        let y = r.center().y;
        match menu {
            Menu::Insert => {
                let items = [
                    ("picture", "Bild", "insert.picture", json!({})),
                    ("table", "Tabelle", "insert.table", json!({"rows": 3, "cols": 3})),
                    ("pageBreak", "Seitenumbruch", "insert.pageBreak", json!({})),
                    ("comment", "Kommentar", "review.newComment", json!({})),
                ];
                let step = r.width() / items.len() as f32;
                for (i, (icon, label, cmd, params)) in items.into_iter().enumerate() {
                    let cell = Rect::from_center_size(pos2(r.min.x + step * (i as f32 + 0.5), y), vec2(step - 4.0, r.height() - 8.0));
                    self.buttons.push((icon, cell.center()));
                    let resp = ui.allocate_rect(cell, Sense::click());
                    if resp.hovered() || resp.is_pointer_button_down_on() {
                        ui.painter().rect_filled(cell, 8.0, accent.gamma_multiply(0.3));
                    }
                    icons::paint(ui.painter(), Rect::from_center_size(cell.center() - vec2(0.0, 10.0), Vec2::splat(24.0)), icon, fg, accent);
                    ui.painter().text(cell.center() + vec2(0.0, 16.0), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), fg);
                    if resp.clicked() {
                        let _ = app.run(cmd, params);
                        return true;
                    }
                }
                false
            }
            Menu::Highlight | Menu::Color => {
                let colors: Vec<(&str, Color32)> = if menu == Menu::Highlight { HIGHLIGHTS.to_vec() } else { COLORS.to_vec() };
                let step = r.width() / colors.len() as f32;
                for (i, (name, c)) in colors.into_iter().enumerate() {
                    let cell = Rect::from_center_size(pos2(r.min.x + step * (i as f32 + 0.5), y), Vec2::splat(36.0));
                    self.buttons.push((name, cell.center()));
                    let resp = ui.allocate_rect(cell, Sense::click());
                    if c == Color32::TRANSPARENT {
                        ui.painter().circle_stroke(cell.center(), 16.0, Stroke::new(2.0, fg));
                        ui.painter().line_segment([cell.left_bottom() + vec2(6.0, -6.0), cell.right_top() + vec2(-6.0, 6.0)], Stroke::new(2.0, fg));
                    } else {
                        ui.painter().circle_filled(cell.center(), 16.0, c);
                        if name == "auto" {
                            ui.painter().text(cell.center(), Align2::CENTER_CENTER, "A", egui::FontId::proportional(14.0), Color32::WHITE);
                        }
                    }
                    if resp.clicked() {
                        let cmd = if menu == Menu::Highlight { "format.highlight" } else { "format.color" };
                        if menu == Menu::Highlight {
                            app.canvas.last_highlight = name.to_string();
                        } else {
                            app.canvas.last_font_color = name.to_string();
                        }
                        let _ = app.run(cmd, json!({"color": name}));
                        return true;
                    }
                }
                false
            }
        }
    }

    /// On a phone showing the full interface: a floating button back to the phone layout.
    fn phone_view_button(&mut self, ui: &mut Ui, area: Rect) {
        let r = Rect::from_min_size(pos2(area.max.x - 150.0 - MARGIN, area.max.y - 44.0 - 36.0), vec2(150.0, 44.0));
        self.buttons.push(("phoneView", r.center()));
        egui::Area::new(egui::Id::unique("phone_view_button")).order(egui::Order::Foreground).fixed_pos(r.min).show(ui.ctx(), |ui| {
            let accent = ui.visuals().selection.bg_fill;
            let resp = ui.allocate_rect(r, Sense::click());
            ui.painter().rect_filled(r, 22.0, accent);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "Handyansicht", egui::FontId::proportional(16.0), Color32::WHITE);
            if resp.clicked() {
                self.all_commands = false;
            }
        });
    }
}

/// A square touch button with one of WordCraft's icons; `active` marks a format that is on.
fn icon_button(ui: &mut Ui, r: Rect, icon: &str, label: &str, active: bool, fg: Color32, accent: Color32) -> egui::Response {
    let resp = ui.allocate_rect(r, Sense::click());
    if active {
        ui.painter().rect_filled(r.shrink(4.0), 10.0, accent.gamma_multiply(0.45));
    } else if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r.shrink(4.0), 10.0, accent.gamma_multiply(0.25));
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(24.0)), icon, fg, accent);
    let label = label.to_string();
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    resp
}

/// A touch button with a text label.
fn text_button(ui: &mut Ui, r: Rect, label: &str, fg: Color32) -> egui::Response {
    let resp = ui.allocate_rect(r, Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r.shrink(4.0), 10.0, ui.visuals().widgets.hovered.bg_fill);
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, egui::FontId::proportional(17.0), fg);
    let label = label.to_string();
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    resp
}
