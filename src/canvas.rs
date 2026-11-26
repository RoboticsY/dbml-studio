use crate::document::{NodeLayout, Positions, DEFAULT_WIDTH};

use crate::{
    inspector::Event,
    model::{Action, Schema, Table},
};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};

const HEADER: f32 = 35.0;
const ROW: f32 = 26.0;
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0, 96, 211);
const INK: Color32 = Color32::from_rgb(33, 34, 40);
const MUTED: Color32 = Color32::from_rgb(89, 92, 102);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Table(usize),
    Column(usize, usize),
    Ref(usize),
    AddTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Table(usize),
    Name(usize, usize),
    Type(usize, usize),
}

impl Field {
    fn text(self, schema: &Schema) -> &str {
        match self {
            Self::Table(t) => &schema.tables[t].name,
            Self::Name(t, c) => &schema.tables[t].columns[c].name,
            Self::Type(t, c) => &schema.tables[t].columns[c].ty,
        }
    }
    fn update(self, schema: &Schema, value: String) -> Action {
        match self {
            Self::Table(t) => Action::RenameTable(t, value),
            Self::Name(t, c) | Self::Type(t, c) => {
                let mut col = schema.tables[t].columns[c].clone();
                match self {
                    Self::Name(..) => col.name = value,
                    _ => col.ty = value,
                }
                Action::UpdateColumn(t, c, col)
            }
        }
    }
    fn editor_rect(self, rects: &[Rect], z: f32) -> Rect {
        let (t, top, height) = match self {
            Self::Table(t) => (t, 0., HEADER),
            Self::Name(t, c) | Self::Type(t, c) => (t, HEADER + ROW * c as f32, ROW),
        };
        let card = rects[t];
        let left = if matches!(self, Self::Type(..)) {
            card.center().x
        } else {
            card.left() + 4. * z
        };
        Rect::from_min_max(
            pos2(left, card.top() + top * z),
            pos2(card.right() - 3. * z, card.top() + (top + height) * z),
        )
    }
}

enum Drag {
    Move(String),
    Resize(String),
}

pub struct Canvas {
    pub positions: Positions,
    zoom: f32,
    pan: Vec2,
    drag: Option<Drag>,
    inline: Option<(Field, String)>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            positions: Positions::new(),
            zoom: 1.0,
            pan: vec2(24.0, 24.0),
            drag: None,
            inline: None,
        }
    }
}

fn table_size(table: &Table, width: f32) -> Vec2 {
    vec2(
        width.max(DEFAULT_WIDTH),
        HEADER + ROW * table.columns.len() as f32 + 5.0,
    )
}

fn resize_hit(point: Pos2, rects: &[Rect]) -> Option<usize> {
    let (i, r) = rects
        .iter()
        .enumerate()
        .rev()
        .find(|(_, r)| r.expand2(vec2(4., 0.)).contains(point))?;
    ((r.right() - point.x).abs() <= 4.).then_some(i)
}

fn table_hit(point: Pos2, rects: &[Rect], zoom: f32, schema: &Schema) -> Option<Pick> {
    let index = rects.iter().rposition(|r| r.contains(point))?;
    let row = ((point.y - rects[index].top()) / zoom - HEADER) / ROW;
    let col = row.floor() as usize;
    if row >= 0.0 && col < schema.tables[index].columns.len() {
        Some(Pick::Column(index, col))
    } else {
        Some(Pick::Table(index))
    }
}

// Place both ports on the right when cards overlap horizontally (including self references).
fn route(a: Rect, ay: f32, b: Rect, by: f32, lane: f32) -> [Pos2; 4] {
    let (start, end, x) = if a.right() < b.left() {
        (a.right(), b.left(), (a.right() + b.left()) * 0.5)
    } else if b.right() < a.left() {
        (a.left(), b.right(), (b.right() + a.left()) * 0.5)
    } else {
        (a.right(), b.right(), a.right().max(b.right()) + lane)
    };
    [pos2(start, ay), pos2(x, ay), pos2(x, by), pos2(end, by)]
}

fn line_distance(point: Pos2, path: &[Pos2; 4]) -> f32 {
    path.windows(2)
        .map(|s| Rect::from_two_pos(s[0], s[1]).distance_to_pos(point))
        .fold(f32::INFINITY, f32::min)
}

pub(crate) fn table_symbol(p: &Painter, r: Rect, color: Color32) {
    let stroke = Stroke::new((r.width() / 10.).max(0.7), color);
    p.rect_stroke(r, 2., stroke, StrokeKind::Inside);
    let y = r.top() + r.height() * 0.35;
    p.hline(r.x_range(), y, stroke);
    p.vline(r.left() + r.width() * 0.38, y..=r.bottom(), stroke);
}

fn card(
    ui: &mut egui::Ui,
    p: &Painter,
    rect: Rect,
    schema: &Schema,
    index: usize,
    selection: Option<Pick>,
    z: f32,
) -> Option<Field> {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let t = &schema.tables[index];
    let keys = &schema.column_keys()[index];
    let selected = matches!(selection, Some(Pick::Table(i) | Pick::Column(i, _)) if i == index);
    p.rect_filled(
        rect.translate(vec2(0., 2.) * z).expand(1. * z),
        11. * z,
        Color32::from_black_alpha(10),
    );
    p.rect_filled(rect, 10. * z, Color32::WHITE);
    let h = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER * z));
    let tile = Rect::from_center_size(h.left_center() + vec2(18., 0.) * z, vec2(21., 21.) * z);
    p.rect_filled(
        tile,
        6. * z,
        if selected {
            ACCENT
        } else {
            Color32::from_rgb(230, 239, 253)
        },
    );
    table_symbol(
        p,
        tile.shrink(4.5 * z),
        if selected { Color32::WHITE } else { ACCENT },
    );
    p.hline(
        (h.left() + 8. * z)..=(h.right() - 8. * z),
        h.bottom(),
        Stroke::new(z, Color32::from_gray(230)),
    );
    let title = Rect::from_min_max(
        pos2(tile.right() + 7. * z, h.top()),
        h.max - vec2(7. * z, 0.),
    );
    let mut field = pointer
        .filter(|p| title.contains(*p))
        .map(|_| Field::Table(index));
    p.with_clip_rect(title).text(
        title.left_center(),
        Align2::LEFT_CENTER,
        &t.name,
        FontId::proportional(15. * z),
        INK,
    );
    let slots = keys
        .iter()
        .map(|k| usize::from(k.primary) + usize::from(k.foreign))
        .max()
        .unwrap_or(0);
    for (i, (c, k)) in t.columns.iter().zip(keys).enumerate() {
        let row = Rect::from_min_size(
            pos2(rect.left(), h.bottom() + ROW * i as f32 * z),
            vec2(rect.width(), ROW * z),
        );
        if !p.clip_rect().intersects(row) {
            continue;
        }
        if Some(Pick::Column(index, i)) == selection {
            p.rect_filled(
                row.shrink2(vec2(3., 1.) * z),
                5. * z,
                Color32::from_rgb(225, 238, 255),
            );
        }
        let mut badge_x = row.left() + 6.0 * z;
        for (visible, badge, role, background, foreground) in [
            (
                k.primary,
                "PK",
                "主キー",
                Color32::from_rgb(255, 225, 158),
                Color32::from_rgb(97, 55, 0),
            ),
            (
                k.foreign,
                "FK",
                "外部キー",
                Color32::from_rgb(222, 222, 251),
                Color32::from_rgb(62, 47, 132),
            ),
        ] {
            if !visible {
                continue;
            }
            let chip = Rect::from_center_size(
                pos2(badge_x + 10.5 * z, row.center().y),
                vec2(21.0, 16.0) * z,
            );
            p.rect_filled(chip, 7.0 * z, background);
            p.text(
                chip.center(),
                Align2::CENTER_CENTER,
                badge,
                FontId::monospace(11.0 * z),
                foreground,
            );
            let response = ui.interact(
                chip.intersect(p.clip_rect()),
                ui.id().with(("column-key", index, i, badge)),
                Sense::hover(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!("{}.{}  {role}", t.name, c.name),
                )
            });
            response.on_hover_text(role);
            badge_x += 24.0 * z;
        }
        let name = row.left_center() + vec2(7.0 + slots as f32 * 24.0, 0.0) * z;
        let right = row.right() - 8. * z;
        let available = right - name.x;
        let mut type_job = egui::text::LayoutJob::simple_singleline(
            c.ty.clone(),
            FontId::monospace(11. * z),
            MUTED,
        );
        type_job.wrap.max_width = available * 0.50;
        type_job.wrap.max_rows = 1;
        let ty = p.layout_job(type_job);
        let type_pos = pos2(right - ty.size().x, row.center().y - ty.size().y * 0.5);
        if let Some(pos) = pointer.filter(|pt| row.contains(*pt) && pt.x >= name.x - 2. * z) {
            field = Some(if pos.x >= type_pos.x - 3. * z {
                Field::Type(index, i)
            } else {
                Field::Name(index, i)
            });
        }
        let mut name_job = egui::text::LayoutJob::simple_singleline(
            c.name.clone(),
            FontId::proportional(13. * z),
            INK,
        );
        name_job.wrap.max_width = (type_pos.x - name.x - 5. * z).max(1.);
        name_job.wrap.max_rows = 1;
        let label = p.layout_job(name_job);
        if ty.elided || label.elided {
            ui.interact(
                Rect::from_min_max(pos2(name.x, row.top()), row.max).intersect(p.clip_rect()),
                ui.id().with(("column-detail", index, i)),
                Sense::hover(),
            )
            .on_hover_text(format!("{}   {}", c.name, c.ty));
        }
        p.galley(pos2(name.x, name.y - label.size().y * 0.5), label, INK);
        p.galley(type_pos, ty, MUTED);
    }
    p.rect_stroke(
        rect,
        10.0 * z,
        Stroke::new(
            if selected { 2.0_f32 } else { 1.0_f32 },
            if selected {
                ACCENT
            } else {
                Color32::from_rgb(205, 208, 217)
            },
        ),
        StrokeKind::Inside,
    );
    field
}

impl Canvas {
    pub fn is_editing(&self) -> bool {
        self.inline.is_some()
    }
    pub fn cancel_edit(&mut self) {
        self.inline = None;
    }
    fn place_new(&mut self, schema: &Schema, width: f32) {
        let mut occupied: Vec<Rect> = schema
            .tables
            .iter()
            .filter_map(|t| {
                self.positions
                    .get(&t.name)
                    .map(|p| Rect::from_min_size(pos2(p.x, p.y), table_size(t, p.width)))
            })
            .collect();
        let cols = ((width + 30.0) / (DEFAULT_WIDTH + 65.0))
            .floor()
            .clamp(1.0, 5.0) as usize;
        for t in &schema.tables {
            if self.positions.contains_key(&t.name) {
                continue;
            }
            let mut y = 0.0;
            let rect = 'rows: loop {
                let mut next_y = f32::INFINITY;
                for i in 0..cols {
                    let rect = Rect::from_min_size(
                        pos2(i as f32 * (DEFAULT_WIDTH + 65.0), y),
                        table_size(t, DEFAULT_WIDTH),
                    );
                    let mut free = true;
                    for other in &occupied {
                        if rect.intersects(other.expand(22.0)) {
                            free = false;
                            next_y = next_y.min(other.bottom() + 44.0);
                        }
                    }
                    if free {
                        break 'rows rect;
                    }
                }
                y = next_y;
            };
            self.positions
                .insert(t.name.clone(), NodeLayout::from([rect.min.x, rect.min.y]));
            occupied.push(rect);
        }
    }

    fn zoom_at(&mut self, factor: f32, anchor: Vec2) {
        let new = (self.zoom * factor).clamp(0.2, 2.7);
        self.pan = anchor - (anchor - self.pan) * (new / self.zoom);
        self.zoom = new;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        schema: &Schema,
        selection: Option<Pick>,
    ) -> Option<Event> {
        let mut result = None;
        let mut fit = false;
        let mut factor = 1.0;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 5))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("＋ テーブルを追加").color(Color32::WHITE),
                            )
                            .fill(ACCENT)
                            .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        result = Some(Pick::AddTable);
                    }
                    ui.separator();
                    if ui.button("−").on_hover_text("縮小").clicked() {
                        factor = 1.0 / 1.2;
                    }
                    ui.monospace(format!("{:.0}%", self.zoom * 100.0));
                    if ui.button("＋").on_hover_text("拡大").clicked() {
                        factor = 1.2;
                    }
                    if ui.button("全体を表示").clicked() {
                        fit = true;
                    }
                })
            });
        let (area, response) = ui.allocate_exact_size(
            ui.available_size().max(vec2(1.0, 1.0)),
            Sense::click_and_drag(),
        );
        self.place_new(schema, area.width());
        let worlds: Vec<_> = schema
            .tables
            .iter()
            .map(|t| {
                let p = self.positions[&t.name];
                Rect::from_min_size(pos2(p.x, p.y), table_size(t, p.width))
            })
            .collect();
        if fit {
            if let Some(bounds) = worlds.iter().copied().reduce(Rect::union) {
                let inner = (area.size() - vec2(40.0, 40.0)).max(vec2(1.0, 1.0));
                self.zoom = (inner.x / bounds.width())
                    .min(inner.y / bounds.height())
                    .clamp(0.2, 1.4);
                self.pan = area.size() * 0.5 - bounds.center().to_vec2() * self.zoom;
            }
        } else if factor != 1.0 {
            self.zoom_at(factor, area.size() * 0.5);
        }

        let pointer = response.hover_pos();
        if let Some(mouse) = pointer {
            let (pinch, wheel) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta.y));
            if pinch != 1.0 || wheel != 0.0 {
                self.zoom_at(
                    if pinch != 1.0 {
                        pinch
                    } else {
                        (wheel * 0.0025).exp()
                    },
                    mouse - area.min,
                );
            }
        }
        let to_screen = |world: Rect, pan: Vec2, zoom: f32| {
            Rect::from_min_size(
                area.min + pan + world.min.to_vec2() * zoom,
                world.size() * zoom,
            )
        };
        let mut rects: Vec<_> = worlds
            .iter()
            .map(|r| to_screen(*r, self.pan, self.zoom))
            .collect();
        if response.drag_started() {
            self.drag = ui.input(|i| i.pointer.press_origin()).and_then(|pos| {
                if !response.drag_started_by(egui::PointerButton::Primary) {
                    return None;
                }
                if let Some(i) = resize_hit(pos, &rects) {
                    self.inline = None;
                    Some(Drag::Resize(schema.tables[i].name.clone()))
                } else if let Some(Pick::Table(i)) = table_hit(pos, &rects, self.zoom, schema) {
                    result = Some(Pick::Table(i));
                    Some(Drag::Move(schema.tables[i].name.clone()))
                } else {
                    None
                }
            });
        }
        if response.dragged() {
            let delta = response.drag_delta();
            match &self.drag {
                Some(gesture) => {
                    let (Drag::Move(name) | Drag::Resize(name)) = gesture;
                    if let Some(i) = schema.tables.iter().position(|t| &t.name == name) {
                        let layout = self.positions.get_mut(name).unwrap();
                        match gesture {
                            Drag::Move(_) => {
                                layout.x += delta.x / self.zoom;
                                layout.y += delta.y / self.zoom;
                                rects[i] = rects[i].translate(delta);
                            }
                            Drag::Resize(_) => {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    layout.width =
                                        ((pos.x - rects[i].left()) / self.zoom).max(DEFAULT_WIDTH);
                                    rects[i].set_width(layout.width * self.zoom);
                                }
                            }
                        }
                    }
                }
                None => {
                    self.pan += delta;
                    for r in &mut rects {
                        *r = r.translate(delta);
                    }
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }

        let z = self.zoom;
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0., Color32::from_rgb(250, 250, 252));
        let grid = (24. * z).max(14.);
        let mut y = area.top() + self.pan.y.rem_euclid(grid);
        while y < area.bottom() {
            let mut x = area.left() + self.pan.x.rem_euclid(grid);
            while x < area.right() {
                painter.circle_filled(pos2(x, y), 0.85, Color32::from_rgb(208, 211, 220));
                x += grid;
            }
            y += grid;
        }
        let endpoint = |(name, col): &(String, String)| {
            let ti = schema.tables.iter().position(|t| &t.name == name)?;
            let ci = schema.tables[ti]
                .columns
                .iter()
                .position(|c| &c.name == col)?;
            Some((
                rects[ti],
                rects[ti].top() + (HEADER + ROW * (0.5 + ci as f32)) * z,
            ))
        };
        let mut line_hit = None;
        let mut closest = 7.0;
        for (i, rel) in schema.refs.iter().enumerate() {
            let (Some((a, ay)), Some((b, by))) = (endpoint(&rel.left), endpoint(&rel.right)) else {
                continue;
            };
            let path = route(a, ay, b, by, (30.0 + 7.0 * (i % 4) as f32) * z);
            let active = selection == Some(Pick::Ref(i));
            let color = if active {
                ACCENT
            } else {
                Color32::from_rgb(110, 125, 149)
            };
            for pair in path.windows(2) {
                painter.line_segment(
                    [pair[0], pair[1]],
                    Stroke::new(if active { 3.0_f32 } else { 1.8_f32 }, color),
                );
            }
            for (port, bend, many) in [
                (path[0], path[1], matches!(rel.kind.as_str(), ">" | "<>")),
                (path[3], path[2], matches!(rel.kind.as_str(), "<" | "<>")),
            ] {
                painter.circle_filled(port, 2.8 * z, color);
                let side = (bend.x - port.x).signum();
                painter.text(
                    port + vec2(9.0 * side, -6.0) * z,
                    Align2::CENTER_CENTER,
                    if many { "N" } else { "1" },
                    FontId::monospace(10.0 * z),
                    color,
                );
            }
            if let Some(p) = pointer {
                let d = line_distance(p, &path);
                if d <= closest {
                    line_hit = Some(Pick::Ref(i));
                    closest = d;
                }
            }
        }
        let mut field = None;
        for (i, rect) in rects.iter().enumerate() {
            if rect.intersects(area) {
                let hovered = card(ui, &painter, *rect, schema, i, selection, z);
                if pointer.is_some_and(|pt| rect.contains(pt)) {
                    field = hovered;
                }
            }
        }
        let edge = pointer.and_then(|pt| resize_hit(pt, &rects));
        if let Some(i) = edge {
            let r = rects[i];
            let center = r.center();
            let half = (r.height() / 2.).min(10.);
            painter.line_segment(
                [
                    pos2(r.right(), center.y - half),
                    pos2(r.right(), center.y + half),
                ],
                Stroke::new(3.0_f32, ACCENT),
            );
        }
        let hit = pointer
            .and_then(|p| table_hit(p, &rects, z, schema))
            .or(line_hit);
        if response.clicked() {
            result = hit;
        }
        if matches!(self.drag, Some(Drag::Resize(_))) || edge.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        } else if pointer.is_some() {
            ui.ctx().set_cursor_icon(match hit {
                _ if field.is_some() && !response.dragged() => CursorIcon::Text,
                Some(Pick::Table(_)) => CursorIcon::Grab,
                Some(_) => CursorIcon::PointingHand,
                None if response.dragged() => CursorIcon::Grabbing,
                None => CursorIcon::Grab,
            });
        }
        response.context_menu(|ui| {
            if ui.button("テーブルを追加").clicked() {
                result = Some(Pick::AddTable);
                ui.close_menu();
            }
        });
        let start = field.filter(|_| edge.is_none() && response.double_clicked());
        if let Some(target) = start {
            self.inline = Some((target, target.text(schema).to_owned()));
        }
        if let Some((target, value)) = &mut self.inline {
            let rect = target.editor_rect(&rects, z).intersect(area);
            if !rect.is_positive() || !ui.is_enabled() {
                self.inline = None;
            } else {
                let id = ui.make_persistent_id("diagram-input");
                let activate = start.is_some();

                let enter =
                    ui.memory(|m| m.has_focus(id)) && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let cancel = ui.input(|i| {
                    i.key_pressed(egui::Key::Escape)
                        || (i.pointer.any_pressed()
                            && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)))
                });
                let input = ui.put(
                    rect,
                    egui::TextEdit::singleline(value)
                        .id(id)
                        .font(FontId::proportional((13. * z).clamp(12., 22.))),
                );
                if activate {
                    input.request_focus();
                    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                    state.clear_undoer();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(value.chars().count()),
                        )));
                    state.store(ui.ctx(), id);
                } else if cancel {
                    self.inline = None;
                } else if enter && input.lost_focus() {
                    input.request_focus();
                    return Some(Event::Apply(target.update(schema, value.clone())));
                }
            }
        }
        result.map(Event::Select)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_key_badges_and_column_name_do_not_overlap() {
        let schema = Schema::parse(
            "Table parent { id int [pk] }\nTable child { owner int [pk, ref: > parent.id] }",
        )
        .unwrap();
        let mut canvas = Canvas::default();
        canvas
            .positions
            .insert("child".into(), [310.0, 70.0].into());
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
            ..Default::default()
        };
        let frame = egui::Context::default().run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| canvas.show(ui, &schema, None));
        });
        let text: Vec<_> = frame
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((
                    t.galley.job.text.as_str(),
                    Rect::from_min_size(t.pos, t.galley.rect.size()),
                )),
                _ => None,
            })
            .collect();
        let owner = text.iter().find(|(s, _)| *s == "owner").unwrap().1;
        let badge = |name| {
            text.iter()
                .find(|(s, r)| {
                    *s == name && r.center().y > owner.top() && r.center().y < owner.bottom()
                })
                .unwrap()
                .1
        };
        assert!(badge("PK").right() < badge("FK").left());
        assert!(badge("FK").right() < owner.left());
        assert_eq!(canvas.positions["child"], [310.0, 70.0].into());
    }

    #[test]
    fn double_click_type_then_replace_and_commit() {
        let schema = Schema::parse("Table item { code varchar(30) [unique] }").unwrap();
        let mut canvas = Canvas::default();
        let ctx = egui::Context::default();
        let mut time = 0.;
        let mut frame = |events| {
            time += 0.04;
            let mut edit = None;
            let output = ctx.run(
                egui::RawInput {
                    time: Some(time),
                    events,
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900., 600.))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        edit = canvas.show(ui, &schema, None);
                    });
                },
            );
            (output, edit)
        };
        let (out, _) = frame(vec![]);
        let pos = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "varchar(30)" => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false, true, false] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        frame(vec![]);
        frame(vec![egui::Event::Text("text".into())]);
        let (_, event) = frame(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        let Some(Event::Apply(Action::UpdateColumn(0, 0, col))) = event else {
            panic!("inline field did not commit")
        };
        assert_eq!(col.ty, "text");
        assert_eq!(col.name, "code");
        assert!(col.unique);
    }

    #[test]
    fn selectable_link_runs_outside_the_cards() {
        let a = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 120.0));
        let b = a.translate(vec2(280.0, 40.0));
        let p = route(a, 50.0, b, 90.0, 30.0);
        assert_eq!(line_distance(pos2(240.0, 70.0), &p), 0.0);
        assert!(line_distance(pos2(257.0, 70.0), &p) > 7.0);
        let self_ref = route(a, 50.0, a, 80.0, 30.0);
        assert_eq!(line_distance(pos2(230.0, 65.0), &self_ref), 0.0);
    }
}
