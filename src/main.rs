#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod canvas;
mod document;
mod format;
mod inspector;
mod model;

use canvas::{Canvas, Pick};
use document::Document;
use eframe::egui::{self, Color32, Key, Modifiers};
use inspector::{Event, Form};
use model::{Action, Relation};
use std::path::{Path, PathBuf};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1370., 830.])
            .with_min_inner_size([880., 520.]),
        ..Default::default()
    };
    eframe::run_native(
        "DBML Studio",
        options,
        Box::new(|cc| {
            let ctx = &cc.egui_ctx;
            ctx.set_visuals(egui::Visuals::light());
            let mut fonts = egui::FontDefinitions::default();
            let dir =
                PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:/Windows".into()))
                    .join("Fonts");
            if let Some(bytes) = ["meiryo.ttc", "YuGothR.ttc", "msgothic.ttc"]
                .iter()
                .find_map(|f| std::fs::read(dir.join(f)).ok())
            {
                fonts
                    .font_data
                    .insert("Japanese".into(), egui::FontData::from_owned(bytes).into());
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .push("Japanese".into());
                }
                ctx.set_fonts(fonts);
            }
            ctx.style_mut(|s| {
                s.spacing.button_padding = egui::vec2(9., 5.);
                s.visuals.panel_fill = Color32::from_rgb(252, 252, 249);
                s.visuals.selection.bg_fill = Color32::from_rgb(176, 220, 215);
                s.visuals.hyperlink_color = Color32::from_rgb(15, 100, 99);
            });
            let mut app = Studio::new();
            for path in std::env::args_os().skip(1) {
                app.open(Path::new(&path))
            }
            Ok(Box::new(app))
        }),
    )
}

#[derive(Clone, Copy)]
enum FileTask {
    New,
    Open,
    CloseTab,
    Exit,
}
enum Dialog {
    Delete(Action, String),
    Leave(FileTask),
}
struct Tab {
    id: u64,
    doc: Document,
    canvas: Canvas,
    form: Form,
    pick: Option<Pick>,
    jump: Option<usize>,
    baseline_on_draw: bool,
    message: String,
    dialog: Option<Dialog>,
    source_visible: bool,
    autosave_at: Option<std::time::Instant>,
    draft: bool,
}
impl Tab {
    fn new(id: u64) -> Self {
        Self {
            id,
            doc: Document::new(),
            canvas: Canvas::default(),
            form: Form::None,
            pick: None,
            jump: None,
            baseline_on_draw: false,
            message: String::new(),
            dialog: None,
            source_visible: false,
            autosave_at: None,
            draft: false,
        }
    }
    fn clear_form(&mut self) {
        self.form = Form::None;
        self.pick = None;
        self.jump = None;
    }
    fn dirty(&self) -> bool {
        self.doc.layout_pending
            || self.doc.text != self.doc.saved
            || self.canvas.positions != self.doc.saved_positions
    }
    fn changed(&mut self) {
        self.autosave_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(1200));
    }
    fn autosave(&mut self) -> Result<(), String> {
        if !self.dirty() {
            return Ok(());
        }
        if let Some(path) = self.doc.path.clone() {
            if self.doc.text == self.doc.saved || self.doc.error.is_some() {
                return self.doc.save_layout(&path, &self.canvas.positions, false);
            }
            self.doc.save(&path, &self.canvas.positions)?;
        } else {
            if self.doc.error.is_some() {
                return Ok(());
            }
            let dir = drafts_directory().ok_or("下書きの保存先が見つかりません。")?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = tempfile::Builder::new()
                .prefix("Draft-")
                .suffix(".dbml")
                .tempfile_in(&dir)
                .map_err(|e| e.to_string())?;
            let (_, path) = file.keep().map_err(|e| e.to_string())?;
            if let Err(e) = self.doc.save(&path, &self.canvas.positions) {
                if self.doc.path.as_ref() != Some(&path) {
                    let _ = std::fs::remove_file(&path);
                }
                return Err(e);
            }
            self.draft = true;
        }
        self.message = if self.draft {
            format!(
                "下書きを自動保存しました。{}",
                self.doc.path.as_ref().unwrap().display()
            )
        } else {
            "自動保存しました。".into()
        };
        Ok(())
    }
    fn title(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("無題 {}", self.id))
    }
    fn select(&mut self, pick: Pick) {
        self.message.clear();
        self.pick = Some(pick);
        self.form = Form::pick(&self.doc.schema, pick);
        if let Pick::Table(i) | Pick::Column(i, _) = pick {
            self.jump = Some(self.doc.schema.tables[i].start);
        }
    }
    fn edit(&mut self, action: Action) {
        let selection = match &action {
            Action::AddTable(_) => Some(Pick::Table(self.doc.schema.tables.len())),
            Action::RenameTable(t, _) | Action::DeleteColumn(t, _) => Some(Pick::Table(*t)),
            Action::AddColumn(t, _) => {
                Some(Pick::Column(*t, self.doc.schema.tables[*t].columns.len()))
            }
            Action::UpdateColumn(t, c, _) | Action::MoveColumn(t, _, c) => {
                Some(Pick::Column(*t, *c))
            }
            Action::AddRef(_) | Action::UpdateRef(_, _) => None,
            _ => None,
        };
        match self.doc.apply(&action, &mut self.canvas.positions) {
            Ok(()) => {
                self.changed();
                self.clear_form();
                self.message.clear();
                let selection = selection.or_else(|| match &action {
                    Action::AddRef(r) | Action::UpdateRef(_, r) => self
                        .doc
                        .schema
                        .refs
                        .iter()
                        .position(|s| s.left == r.left && s.right == r.right && s.kind == r.kind)
                        .map(Pick::Ref),
                    _ => None,
                });
                if let Some(p) = selection {
                    self.select(p)
                }
            }
            Err(e) => self.message = e,
        }
    }
    fn request_edit(&mut self, action: Action) {
        let s = &self.doc.schema;
        let (name, related) = match &action {
            Action::DeleteRef(i) => (ref_label(&s.refs[*i]), vec![]),
            Action::DeleteTable(t) | Action::DeleteColumn(t, _) => {
                let table = &s.tables[*t];
                let column = if let Action::DeleteColumn(_, c) = &action {
                    Some(&table.columns[*c].name)
                } else {
                    None
                };
                let matches =
                    |e: &(String, String)| e.0 == table.name && column.is_none_or(|c| c == &e.1);
                let refs = s
                    .refs
                    .iter()
                    .filter(|r| matches(&r.left) || matches(&r.right))
                    .map(ref_label)
                    .collect();
                (
                    column.map_or_else(|| table.name.clone(), |c| format!("{}.{}", table.name, c)),
                    refs,
                )
            }
            _ => {
                self.edit(action);
                return;
            }
        };
        if let Err(e) = s.edit(&self.doc.text, &action) {
            self.message = e;
            return;
        }
        let mut msg = format!("{name} を削除しますか？");
        if !related.is_empty() {
            msg.push_str("\n次の参照も削除します。\n");
            msg.push_str(&related.join("\n"));
        }
        self.dialog = Some(Dialog::Delete(action, msg));
    }
    fn history(&mut self, redo: bool) {
        self.doc.history(&mut self.canvas.positions, redo);
        self.changed();
        self.clear_form();
        self.message.clear();
    }
    fn source(&mut self, ui: &mut egui::Ui) {
        ui.strong("DBML");
        ui.separator();
        egui::ScrollArea::both()
            .id_salt(("source-scroll", self.id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let old = self.doc.snapshot(&self.canvas.positions);
                let mut out = egui::TextEdit::multiline(&mut self.doc.text)
                    .id(egui::Id::new(("dbml-text", self.id)))
                    .code_editor()
                    .hint_text("Table users {\n  id int [pk]\n}")
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .show(ui);
                if out.response.changed() {
                    self.doc.remember(old);
                    self.changed();
                    self.doc.parse();
                    self.clear_form();
                    self.message.clear();
                }
                if let Some(pos) = self.jump.take() {
                    let index = self.doc.text.get(..pos).map_or(0, |p| p.chars().count());
                    let cursor = egui::text::CCursor::new(index);
                    let rect = out
                        .galley
                        .pos_from_cursor(&out.galley.from_ccursor(cursor))
                        .translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    out.state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
                    out.state.store(ui.ctx(), out.response.id);
                }
            });
    }
}
fn drafts_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("DBML Studio").join("Drafts"))
}
fn ref_label(r: &Relation) -> String {
    format!(
        "{}.{} {} {}.{}",
        r.left.0, r.left.1, r.kind, r.right.0, r.right.1
    )
}
impl Tab {
    fn draw(&mut self, ctx: &egui::Context) {
        let title = self.title();
        let dirty = self.dirty();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if let Some(e) = &self.doc.error {
                ui.colored_label(
                    Color32::from_rgb(159, 55, 38),
                    format!("図の編集を一時停止しています。{e}"),
                );
            }
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
            ui.horizontal(|ui| {
                ui.strong(&title);
                ui.separator();
                ui.label(format!(
                    "{} テーブル　{} 参照",
                    self.doc.schema.tables.len(),
                    self.doc.schema.refs.len()
                ));
                if dirty {
                    ui.label("未保存");
                }
            });
        });
        if self.source_visible {
            egui::TopBottomPanel::bottom(egui::Id::new(("source", self.id)))
                .resizable(true)
                .default_height(210.)
                .height_range(100.0..=460.)
                .show(ctx, |ui| self.source(ui));
        }
        egui::SidePanel::right(egui::Id::new(("inspector", self.id)))
            .default_width(226.)
            .width_range(190.0..=450.)
            .show(ctx, |ui| {
                ui.strong("プロパティ");
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("inspector-scroll", self.id))
                    .show(ui, |ui| {
                        let event = ui
                            .add_enabled_ui(self.doc.error.is_none(), |ui| {
                                self.form.show(ui, &self.doc.schema, self.id)
                            })
                            .inner;
                        match event {
                            Some(Event::Apply(a)) => self.request_edit(a),
                            Some(Event::Select(p)) => self.select(p),
                            None => {}
                        }
                    });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| {
                let valid = self.doc.error.is_none();
                let prior = ctx
                    .input(|i| i.pointer.any_down() || i.pointer.any_released())
                    .then(|| self.canvas.positions.clone());
                let pick = ui
                    .add_enabled_ui(valid, |ui| {
                        self.canvas.show(ui, &self.doc.schema, self.pick)
                    })
                    .inner;
                if self.baseline_on_draw {
                    self.doc.saved_positions = self.canvas.positions.clone();
                    self.baseline_on_draw = false;
                } else if prior.is_some_and(|p| p != self.canvas.positions) {
                    self.changed();
                }
                if valid {
                    if let Some(p) = pick {
                        self.select(p)
                    }
                }
            });
    }
}

struct Studio {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    closing: bool,
}
impl Studio {
    fn new() -> Self {
        Self {
            tabs: vec![Tab::new(1)],
            active: 0,
            next_id: 2,
            closing: false,
        }
    }
    fn add(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id));
        self.active = self.tabs.len() - 1;
        self.active
    }
    fn owner(&self, path: &Path) -> Option<usize> {
        let target = std::fs::canonicalize(path).ok();
        self.tabs.iter().position(|tab| {
            tab.doc.path.as_deref().is_some_and(|p| {
                p == path
                    || target
                        .as_ref()
                        .is_some_and(|t| std::fs::canonicalize(p).as_ref().ok() == Some(t))
            })
        })
    }
    fn open(&mut self, path: &Path) {
        if let Some(i) = self.owner(path) {
            self.active = i;
            return;
        }
        match Document::open(path) {
            Ok((doc, positions, warning)) => {
                let tab = &self.tabs[self.active];
                let placeholder = self.tabs.len() == 1
                    && tab.id == 1
                    && tab.doc.path.is_none()
                    && !tab.dirty()
                    && !tab.doc.has_history()
                    && matches!(tab.form, Form::None);
                if !placeholder {
                    self.add();
                }
                let tab = &mut self.tabs[self.active];
                tab.baseline_on_draw = doc.error.is_none();
                tab.doc = doc;
                tab.canvas.positions = positions;
                tab.message = warning.unwrap_or_default();
            }
            Err(err) => {
                self.tabs[self.active].message = format!("ファイルを開けませんでした。{err}")
            }
        }
    }
    fn file(&mut self, ctx: &egui::Context, task: FileTask, confirmed: bool) {
        match task {
            FileTask::New => {
                self.add();
            }
            FileTask::Open => {
                let mut picker = rfd::FileDialog::new().add_filter("DBML", &["dbml"]);
                if let Some(dir) = self.tabs[self.active]
                    .doc
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
                    .or_else(drafts_directory)
                    .filter(|dir| dir.is_dir())
                {
                    picker = picker.set_directory(dir);
                }
                if let Some(paths) = picker.pick_files() {
                    for p in paths {
                        self.open(&p)
                    }
                }
            }
            FileTask::CloseTab => {
                if !confirmed && self.tabs[self.active].dirty() {
                    self.tabs[self.active].dialog = Some(Dialog::Leave(task));
                    return;
                }
                self.tabs.remove(self.active);
                if self.tabs.is_empty() {
                    self.add();
                } else {
                    self.active = self.active.min(self.tabs.len() - 1);
                }
            }
            FileTask::Exit => {
                let start = if confirmed { self.active + 1 } else { 0 };
                if let Some(index) = (start..self.tabs.len()).find(|&i| self.tabs[i].dirty()) {
                    self.active = index;
                    self.tabs[index].dialog = Some(Dialog::Leave(task));
                } else {
                    self.closing = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
    fn tab_bar(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, tab) in self.tabs.iter().enumerate() {
                            ui.push_id(tab.id, |ui| {
                                let mut name = tab.title();
                                if tab.dirty() {
                                    name.push_str(" *");
                                }
                                let response = ui.selectable_label(i == self.active, name);
                                if response.clicked() {
                                    self.active = i;
                                }
                                if let Some(path) = &tab.doc.path {
                                    response.on_hover_text(path.display().to_string());
                                }
                                if ui.small_button("×").on_hover_text("タブを閉じる").clicked()
                                {
                                    self.active = i;
                                    close = true;
                                }
                                ui.separator();
                            });
                        }
                    });
                });
        });
        if close {
            self.file(ctx, FileTask::CloseTab, false);
        }
    }
    fn save(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.tabs[self.active].draft {
            None
        } else {
            self.tabs[self.active].doc.path.clone()
        }
        .or_else(|| {
            let name = self.tabs[self.active]
                .doc
                .path
                .as_deref()
                .and_then(Path::file_name)
                .and_then(|n| n.to_str())
                .unwrap_or("schema.dbml");
            rfd::FileDialog::new()
                .add_filter("DBML", &["dbml"])
                .set_file_name(name)
                .save_file()
        });
        let Some(path) = path else { return false };
        if self.owner(&path).is_some_and(|i| i != self.active) {
            self.tabs[self.active].message =
                "このファイルは別のタブで開いています。別の保存先を選んでください。".into();
            return false;
        }
        let tab = &mut self.tabs[self.active];
        match tab.doc.save(&path, &tab.canvas.positions) {
            Ok(()) => {
                tab.autosave_at = None;
                tab.draft = false;
                tab.message = "保存しました。".into();
                true
            }
            Err(err) => {
                tab.message = err;
                false
            }
        }
    }
    fn modal(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.tabs[self.active].dialog.take() else {
            return;
        };
        let question = format!(
            "「{}」の変更を保存しますか？",
            self.tabs[self.active].title()
        );
        let choice = egui::Modal::new("confirm".into())
            .show(ctx, |ui| {
                ui.set_max_width(550.);
                let text = match &dialog {
                    Dialog::Delete(_, msg) => msg.as_str(),
                    Dialog::Leave(_) => &question,
                };
                egui::ScrollArea::vertical()
                    .max_height(270.)
                    .show(ui, |ui| ui.label(text));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("キャンセル").clicked() {
                        return 1;
                    }
                    match &dialog {
                        Dialog::Delete(..) => {
                            if ui.button("削除する").clicked() {
                                return 2;
                            }
                        }
                        Dialog::Leave(..) => {
                            if ui.button("保存せずに続ける").clicked() {
                                return 2;
                            }
                            if ui.button("保存して続ける").clicked() {
                                return 3;
                            }
                        }
                    };
                    0
                })
                .inner
            })
            .inner;
        match choice {
            0 if !ctx.input(|i| i.key_pressed(Key::Escape)) => {
                self.tabs[self.active].dialog = Some(dialog)
            }
            2 | 3 => match dialog {
                Dialog::Delete(a, _) => self.tabs[self.active].edit(a),
                Dialog::Leave(t) => {
                    if choice == 2 || self.save(false) {
                        self.file(ctx, t, true)
                    } else {
                        self.tabs[self.active].dialog = Some(Dialog::Leave(t));
                    }
                }
            },
            _ => {}
        }
    }
}
impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.closing
            && self.tabs.iter().any(Tab::dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.file(ctx, FileTask::Exit, false);
        }
        let tab = &self.tabs[self.active];
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — DBML Studio",
            tab.title(),
            if tab.dirty() { " *" } else { "" }
        )));
        if self.tabs[self.active].dialog.is_none() {
            let key = |m, k| ctx.input_mut(|i| i.consume_key(m, k));
            if key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
                self.save(true);
            } else if key(Modifiers::COMMAND, Key::S) {
                self.save(false);
            }
            if key(Modifiers::COMMAND, Key::O) {
                self.file(ctx, FileTask::Open, false)
            }
            if key(Modifiers::COMMAND, Key::N) {
                self.file(ctx, FileTask::New, false)
            }
            if key(Modifiers::COMMAND, Key::W) {
                self.file(ctx, FileTask::CloseTab, false);
            }
            let document_undo = !inspector::editing_property(ctx, self.tabs[self.active].id);
            if document_undo
                && (key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
                    || key(Modifiers::COMMAND, Key::Y))
            {
                self.tabs[self.active].history(true)
            } else if document_undo && key(Modifiers::COMMAND, Key::Z) {
                self.tabs[self.active].history(false)
            }
        }
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(4.);
            ui.horizontal_wrapped(|ui| {
                ui.heading("DBML Studio");
                ui.separator();
                if ui.button("新規").clicked() {
                    self.file(ctx, FileTask::New, false)
                }
                if ui.button("開く").clicked() {
                    self.file(ctx, FileTask::Open, false)
                }
                if ui.button("保存").clicked() {
                    self.save(false);
                }
                if ui.button("別名で保存").clicked() {
                    self.save(true);
                }
                ui.separator();
                if ui.button("元に戻す").clicked() {
                    self.tabs[self.active].history(false)
                }
                if ui.button("やり直す").clicked() {
                    self.tabs[self.active].history(true)
                }
                ui.toggle_value(&mut self.tabs[self.active].source_visible, "DBML テキスト");
            });
            ui.add_space(4.);
        });
        self.tab_bar(ctx);
        self.tabs[self.active].draw(ctx);
        self.autosave_due(ctx);
        self.modal(ctx);
    }
}
impl Studio {
    fn autosave_due(&mut self, ctx: &egui::Context) {
        if self
            .tabs
            .iter()
            .any(|tab| matches!(tab.dialog, Some(Dialog::Leave(_))))
        {
            return;
        }
        let now = std::time::Instant::now();
        for tab in &mut self.tabs {
            if let Some(deadline) = tab.autosave_at {
                if deadline <= now {
                    tab.autosave_at = None;
                    if let Err(e) = tab.autosave() {
                        tab.message = format!("自動保存できませんでした。{e}");
                    }
                } else {
                    ctx.request_repaint_after(deadline - now);
                }
            }
        }
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;

    #[test]
    fn autosave_checks_inactive_documents_and_preserves_unrelated_source_on_drag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shop.dbml");
        let original = "Table shop{id int[pk]}";
        std::fs::write(&path, original).unwrap();
        let mut app = Studio::new();
        app.open(&path);
        let tab = &mut app.tabs[0];
        tab.canvas.positions.insert("shop".into(), [80., 50.]);
        tab.autosave().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(Document::open(&path).unwrap().1["shop"], [80., 50.]);
        tab.doc.text = "Table shop{id int[null, not null]}".into();
        tab.doc.parse();
        assert!(tab.doc.error.is_some());
        assert!(tab.doc.save(&path, &tab.canvas.positions).is_err());
        tab.autosave().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        tab.doc.text = "Table shop{id int[pk] name text}".into();
        tab.doc.parse();
        tab.autosave_at = Some(std::time::Instant::now());
        app.add();
        app.autosave_due(&egui::Context::default());
        let expected = "Table shop {\n  id int [pk]\n  name text\n}\n";
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(app.tabs[0].doc.text, expected);
        assert!(!app.tabs[0].dirty());
    }
    #[test]
    fn open_switch_save_and_close_keep_documents_separate() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("first.dbml");
        let b = dir.path().join("second.dbml");
        std::fs::write(&a, "Table first {}").unwrap();
        std::fs::write(&b, "Table second {}").unwrap();
        let mut app = Studio::new();
        app.open(&a);
        app.tabs[0].edit(Action::AddTable("pending".into()));
        app.open(&b);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, 1);
        assert!(!app.tabs[1].source_visible);
        app.tabs[1].edit(Action::AddTable("only_second".into()));
        app.open(&a);
        assert_eq!(app.active, 0);
        assert!(app.tabs[0].doc.text.contains("pending"));
        app.tabs[0].history(false);
        assert_eq!(app.tabs[0].doc.text, "Table first {}");
        assert!(app.tabs[1].dirty());
        app.tabs[0].history(true);
        let ctx = egui::Context::default();
        app.file(&ctx, FileTask::Exit, false);
        assert!(matches!(
            app.tabs[0].dialog.take(),
            Some(Dialog::Leave(FileTask::Exit))
        ));
        app.file(&ctx, FileTask::Exit, true);
        assert_eq!(app.active, 1);
        assert!(matches!(
            app.tabs[1].dialog.take(),
            Some(Dialog::Leave(FileTask::Exit))
        ));
        assert!(app.save(false));
        assert!(std::fs::read_to_string(&b).unwrap().contains("only_second"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "Table first {}");
        app.file(&ctx, FileTask::CloseTab, false);
        assert_eq!(app.tabs.len(), 1);
        app.file(&ctx, FileTask::CloseTab, false);
        assert!(matches!(
            app.tabs[0].dialog.take(),
            Some(Dialog::Leave(FileTask::CloseTab))
        ));
        app.file(&ctx, FileTask::CloseTab, true);
        assert!(app.tabs[0].doc.path.is_none());
    }
}
