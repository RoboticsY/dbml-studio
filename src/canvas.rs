use std::collections::BTreeMap;

use crate::model::{Schema, Table};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};

const WIDTH: f32 = 208.0;
const HEADER: f32 = 35.0;
const ROW: f32 = 26.0;
const ACCENT: Color32 = Color32::from_rgb(18, 107, 104);
const INK: Color32 = Color32::from_rgb(33, 50, 65);
const MUTED: Color32 = Color32::from_rgb(78, 97, 111);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Table(usize),
    Column(usize, usize),
    Ref(usize),
    AddTable,
}

pub struct Canvas {
    pub positions: BTreeMap<String, [f32; 2]>,
    zoom: f32,
    pan: Vec2,
    moving: Option<String>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            positions: BTreeMap::new(),
            zoom: 1.0,
            pan: vec2(24.0, 24.0),
            moving: None,
        }
    }
}

fn table_size(table: &Table) -> Vec2 {
    vec2(WIDTH, HEADER + ROW * table.columns.len() as f32 + 5.0)
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

fn card(
    ui: &mut egui::Ui,
    p: &Painter,
    rect: Rect,
    schema: &Schema,
    index: usize,
    selection: Option<Pick>,
    z: f32,
) {
    let t = &schema.tables[index];
    let keys = &schema.column_keys()[index];
    let selected = matches!(selection, Some(Pick::Table(i) | Pick::Column(i, _)) if i == index);
    p.rect_filled(
        rect.translate(vec2(0.0, 2.0) * z),
        5.0 * z,
        Color32::from_black_alpha(15),
    );
    p.rect_filled(rect, 5.0 * z, Color32::WHITE);
    let h = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER * z));
    p.rect_filled(
        h,
        4.0 * z,
        if selected {
            ACCENT
        } else {
            Color32::from_rgb(40, 63, 79)
        },
    );
    let title = h.shrink2(vec2(11.0, 1.0) * z);
    p.with_clip_rect(title).text(
        title.left_center(),
        Align2::LEFT_CENTER,
        &t.name,
        FontId::proportional(15.0 * z),
        Color32::WHITE,
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
            p.rect_filled(row, 0.0, Color32::from_rgb(218, 240, 236));
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
            p.rect_filled(chip, 3.0 * z, background);
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
        let split = row.left() + row.width() * 0.70;
        p.with_clip_rect(Rect::from_min_max(
            row.min,
            pos2(split - 4.0 * z, row.bottom()),
        ))
        .text(
            name,
            Align2::LEFT_CENTER,
            &c.name,
            FontId::proportional(13.0 * z),
            INK,
        );
        p.with_clip_rect(Rect::from_min_max(
            pos2(split, row.top()),
            row.max - vec2(7.0 * z, 0.0),
        ))
        .text(
            row.right_center() - vec2(8.0 * z, 0.0),
            Align2::RIGHT_CENTER,
            &c.ty,
            FontId::monospace(11.0 * z),
            MUTED,
        );
    }
    p.rect_stroke(
        rect,
        5.0 * z,
        Stroke::new(
            if selected { 2.0_f32 } else { 1.0_f32 },
            if selected {
                ACCENT
            } else {
                Color32::from_rgb(178, 191, 200)
            },
        ),
        StrokeKind::Inside,
    );
}

impl Canvas {
    fn place_new(&mut self, schema: &Schema, width: f32) {
        let mut occupied: Vec<Rect> = schema
            .tables
            .iter()
            .filter_map(|t| {
                self.positions
                    .get(&t.name)
                    .map(|p| Rect::from_min_size(Pos2::from(*p), table_size(t)))
            })
            .collect();
        let cols = ((width + 30.0) / (WIDTH + 65.0)).floor().clamp(1.0, 5.0) as usize;
        for t in &schema.tables {
            if self.positions.contains_key(&t.name) {
                continue;
            }
            let mut y = 0.0;
            let rect = 'rows: loop {
                let mut next_y = f32::INFINITY;
                for i in 0..cols {
                    let rect =
                        Rect::from_min_size(pos2(i as f32 * (WIDTH + 65.0), y), table_size(t));
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
            self.positions.insert(t.name.clone(), rect.min.into());
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
    ) -> Option<Pick> {
        let mut result = None;
        let mut fit = false;
        let mut factor = 1.0;
        ui.horizontal_wrapped(|ui| {
            if ui.button("＋ テーブルを追加").clicked() {
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
        });
        ui.add_space(4.0);
        let (area, response) = ui.allocate_exact_size(
            ui.available_size().max(vec2(1.0, 1.0)),
            Sense::click_and_drag(),
        );
        self.place_new(schema, area.width());
        let worlds: Vec<_> = schema
            .tables
            .iter()
            .map(|t| Rect::from_min_size(Pos2::from(self.positions[&t.name]), table_size(t)))
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
            self.moving = ui.input(|i| i.pointer.press_origin()).and_then(|pos| {
                if let Some(Pick::Table(index)) = table_hit(pos, &rects, self.zoom, schema) {
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        result = Some(Pick::Table(index));
                        return Some(schema.tables[index].name.clone());
                    }
                }
                None
            });
        }
        if response.dragged() {
            let delta = response.drag_delta();
            if let Some(name) = &self.moving {
                if let Some(index) = schema.tables.iter().position(|t| &t.name == name) {
                    let p = self.positions.get_mut(name).unwrap();
                    p[0] += delta.x / self.zoom;
                    p[1] += delta.y / self.zoom;
                    rects[index] = rects[index].translate(delta);
                }
            } else {
                self.pan += delta;
                for rect in &mut rects {
                    *rect = rect.translate(delta);
                }
            }
        }
        if response.drag_stopped() {
            self.moving = None;
        }

        let z = self.zoom;
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 4.0, Color32::from_rgb(243, 247, 249));
        let grid = (24.0 * z).max(10.0);
        let grid_stroke = Stroke::new(0.6_f32, Color32::from_rgb(219, 228, 233));
        let mut x = area.left() + self.pan.x.rem_euclid(grid);
        while x < area.right() {
            painter.vline(x, area.y_range(), grid_stroke);
            x += grid;
        }
        let mut y = area.top() + self.pan.y.rem_euclid(grid);
        while y < area.bottom() {
            painter.hline(area.x_range(), y, grid_stroke);
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
                Color32::from_rgb(92, 119, 135)
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
        for (i, rect) in rects.iter().enumerate() {
            if rect.intersects(area) {
                card(ui, &painter, *rect, schema, i, selection, z);
            }
        }
        let hit = pointer
            .and_then(|p| table_hit(p, &rects, z, schema))
            .or(line_hit);
        if response.clicked() {
            result = hit;
        }
        if pointer.is_some() {
            ui.ctx().set_cursor_icon(match hit {
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
        result
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
        canvas.positions.insert("child".into(), [310.0, 70.0]);
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
        assert_eq!(canvas.positions["child"], [310.0, 70.0]);
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
