use crate::{
    canvas::Pick,
    model::{Action, Column, Relation, Schema},
};
use eframe::egui::{self, Ui};

#[derive(Clone)]
pub enum Form {
    None,
    Table(Option<usize>, String),
    Column(usize, Option<usize>, Column),
    Relation(Option<usize>, Relation),
}

pub enum Event {
    Apply(Action),
    Select(Pick),
}

fn field(ui: &mut Ui, tab: u64, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .id(egui::Id::new(("property", tab, label)))
            .desired_width(f32::INFINITY),
    );
}

pub fn editing_property(ctx: &egui::Context, tab: u64) -> bool {
    ["テーブル名", "カラム名", "データ型", "既定値（DBML 式）"]
        .iter()
        .any(|label| ctx.memory(|m| m.has_focus(egui::Id::new(("property", tab, label)))))
}
impl Form {
    pub fn pick(schema: &Schema, pick: Pick) -> Self {
        match pick {
            Pick::Table(i) => Self::Table(Some(i), schema.tables[i].name.clone()),
            Pick::Column(t, c) => Self::Column(t, Some(c), schema.tables[t].columns[c].clone()),
            Pick::Ref(i) => Self::Relation(Some(i), schema.refs[i].clone()),
            Pick::AddTable => Self::Table(None, String::new()),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, schema: &Schema, tab: u64) -> Option<Event> {
        use Event::{Apply, Select};
        ui.spacing_mut().item_spacing.y = 8.0;
        let result = match self {
            Self::None => {
                ui.label("図でテーブル、カラム、参照線を選んでください。");
                None
            }
            Self::Table(index, name) => {
                field(ui, tab, "テーブル名", name);
                if ui
                    .button(if index.is_some() {
                        "変更を適用"
                    } else {
                        "テーブルを作成"
                    })
                    .clicked()
                {
                    return Some(Apply(match index {
                        Some(i) => Action::RenameTable(*i, name.clone()),
                        None => Action::AddTable(name.clone()),
                    }));
                }
                if let Some(i) = *index {
                    ui.separator();
                    ui.strong("カラム");
                    for (c, col) in schema.tables[i].columns.iter().enumerate() {
                        if ui.selectable_label(false, &col.name).clicked() {
                            return Some(Select(Pick::Column(i, c)));
                        }
                    }
                    if ui.button("＋ カラムを追加").clicked() {
                        *self = Self::Column(
                            i,
                            None,
                            Column {
                                ty: "int".into(),
                                ..Default::default()
                            },
                        );
                        return None;
                    }
                    ui.separator();
                    if ui.button("テーブルを削除…").clicked() {
                        return Some(Apply(Action::DeleteTable(i)));
                    }
                }
                None
            }
            Self::Column(t, index, col) => {
                if ui.link(format!("‹ {}", schema.tables[*t].name)).clicked() {
                    return Some(Select(Pick::Table(*t)));
                }
                field(ui, tab, "カラム名", &mut col.name);
                field(ui, tab, "データ型", &mut col.ty);
                ui.checkbox(&mut col.pk, "主キー");
                ui.checkbox(&mut col.required, "NOT NULL");
                ui.checkbox(&mut col.unique, "一意制約");
                field(ui, tab, "既定値（DBML 式）", &mut col.default);
                if ui
                    .button(if index.is_some() {
                        "変更を適用"
                    } else {
                        "カラムを作成"
                    })
                    .clicked()
                {
                    return Some(Apply(match index {
                        Some(c) => Action::UpdateColumn(*t, *c, col.clone()),
                        None => Action::AddColumn(*t, col.clone()),
                    }));
                }
                if let Some(c) = *index {
                    ui.separator();
                    let moved = ui
                        .horizontal(|ui| {
                            if ui.add_enabled(c > 0, egui::Button::new("上へ")).clicked() {
                                Some(c - 1)
                            } else if ui
                                .add_enabled(
                                    c + 1 < schema.tables[*t].columns.len(),
                                    egui::Button::new("下へ"),
                                )
                                .clicked()
                            {
                                Some(c + 1)
                            } else {
                                None
                            }
                        })
                        .inner;
                    if let Some(to) = moved {
                        return Some(Apply(Action::MoveColumn(*t, c, to)));
                    }
                    if ui.button("このカラムから参照を追加").clicked() {
                        let endpoint = (
                            schema.tables[*t].name.clone(),
                            schema.tables[*t].columns[c].name.clone(),
                        );
                        *self = Self::Relation(
                            None,
                            Relation {
                                left: endpoint,
                                right: Default::default(),
                                kind: ">".into(),
                            },
                        );
                        return None;
                    }
                    if ui.button("カラムを削除…").clicked() {
                        return Some(Apply(Action::DeleteColumn(*t, c)));
                    }
                }
                None
            }
            Self::Relation(index, r) => {
                endpoint(ui, "左側", &mut r.left, schema);
                egui::ComboBox::from_label("関係")
                    .selected_text(&r.kind)
                    .show_ui(ui, |ui| {
                        for (k, label) in [
                            (">", "多対一  >"),
                            ("<", "一対多  <"),
                            ("-", "一対一  -"),
                            ("<>", "多対多  <>"),
                        ] {
                            ui.selectable_value(&mut r.kind, k.into(), label);
                        }
                    });
                endpoint(ui, "右側", &mut r.right, schema);
                if ui
                    .button(if index.is_some() {
                        "変更を適用"
                    } else {
                        "参照を作成"
                    })
                    .clicked()
                {
                    return Some(Apply(match index {
                        Some(i) => Action::UpdateRef(*i, r.clone()),
                        None => Action::AddRef(r.clone()),
                    }));
                }
                if let Some(i) = *index {
                    if ui.button("参照を削除…").clicked() {
                        return Some(Apply(Action::DeleteRef(i)));
                    }
                }
                None
            }
        };
        if schema.tables.iter().any(|t| !t.columns.is_empty())
            && !matches!(self, Self::Relation(..))
        {
            ui.separator();
            if ui.button("＋ 参照を追加").clicked() {
                *self = Self::Relation(
                    None,
                    Relation {
                        kind: ">".into(),
                        ..Default::default()
                    },
                );
            }
        }
        result
    }
}

fn endpoint(ui: &mut Ui, label: &str, point: &mut (String, String), schema: &Schema) {
    ui.label(label);
    let current = if point.0.is_empty() {
        "カラムを選択".into()
    } else {
        format!("{}.{}", point.0, point.1)
    };
    egui::ComboBox::from_id_salt(label)
        .selected_text(current)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for t in &schema.tables {
                for c in &t.columns {
                    ui.selectable_value(
                        point,
                        (t.name.clone(), c.name.clone()),
                        format!("{}.{}", t.name, c.name),
                    );
                }
            }
        });
}
