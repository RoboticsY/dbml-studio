use crate::model::{Action, Schema};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub type Positions = BTreeMap<String, [f32; 2]>;

#[derive(Clone)]
pub struct Snapshot {
    pub text: String,
    pub positions: Positions,
}
pub struct Document {
    pub text: String,
    pub schema: Schema,
    pub error: Option<String>,
    pub path: Option<PathBuf>,
    pub saved: String,
    pub saved_positions: Positions,
    pub bom: bool,
    pub layout_pending: bool,
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            schema: Schema::parse("").expect("empty schema"),
            error: None,
            path: None,
            saved: String::new(),
            saved_positions: Positions::new(),
            bom: false,
            layout_pending: false,
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }
    pub fn open(path: &Path) -> Result<(Self, Positions, Option<String>), String> {
        let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut doc = Self::new();
        doc.bom = raw.starts_with('\u{feff}');
        doc.text = raw.trim_start_matches('\u{feff}').into();
        doc.saved = doc.text.clone();
        doc.path = Some(path.into());
        doc.parse();
        let layout = fs::read(sidecar(path));
        let (positions, warning) = match layout {
            Ok(bytes) => match serde_json::from_slice::<Positions>(&bytes) {
                Ok(p) => (p, None),
                Err(e) => (
                    Positions::new(),
                    Some(format!("配置ファイルを読み込めませんでした。{e}")),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Positions::new(), None),
            Err(e) => (Positions::new(), Some(e.to_string())),
        };
        doc.saved_positions = positions.clone();
        Ok((doc, positions, warning))
    }
    pub fn snapshot(&self, positions: &Positions) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            positions: positions.clone(),
        }
    }
    pub fn remember(&mut self, s: Snapshot) {
        if self.undo.len() >= 100 {
            self.undo.pop_front();
        }
        self.undo.push_back(s);
        self.redo.clear();
    }
    pub fn has_history(&self) -> bool {
        !self.undo.is_empty() || !self.redo.is_empty()
    }
    pub fn parse(&mut self) {
        match Self::checked(&self.text) {
            Ok(schema) => {
                self.schema = schema;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn checked(text: &str) -> Result<Schema, String> {
        let schema = Schema::parse(text)?;
        crate::format::lint(&schema, text)?;
        Ok(schema)
    }
    pub fn apply(&mut self, action: &Action, positions: &mut Positions) -> Result<(), String> {
        if let Some(err) = &self.error {
            return Err(err.clone());
        }
        let new = self.schema.edit(&self.text, action)?;
        if new == self.text {
            return Ok(());
        }
        let schema = Self::checked(&new)?;
        self.remember(self.snapshot(positions));
        match action {
            Action::RenameTable(t, _) => {
                if let Some(pos) = positions.remove(&self.schema.tables[*t].name) {
                    positions.insert(schema.tables[*t].name.clone(), pos);
                }
            }
            Action::DeleteTable(t) => {
                positions.remove(&self.schema.tables[*t].name);
            }
            _ => {}
        }
        self.schema = schema;
        self.text = new;
        Ok(())
    }
    pub fn history(&mut self, positions: &mut Positions, redo: bool) {
        let current = self.snapshot(positions);
        let previous = if redo {
            self.redo.pop()
        } else {
            self.undo.pop_back()
        };
        if let Some(s) = previous {
            if redo {
                self.undo.push_back(current)
            } else {
                self.redo.push(current)
            }
            self.text = s.text;
            *positions = s.positions;
            self.parse();
        }
    }
    pub fn save(&mut self, path: &Path, positions: &Positions) -> Result<(), String> {
        let formatted = crate::format::formatted(&self.text)?;
        let schema = Self::checked(&formatted)?;
        let same = self.path.as_deref() == Some(path);
        if same {
            let prev = fs::read_to_string(path).map_err(|e| e.to_string())?;
            if prev.strip_prefix('\u{feff}').unwrap_or(&prev) != self.saved {
                return Err("ファイルが外部で変更されました。別名で保存してください。".into());
            }
        }
        if !same || formatted != self.saved {
            let mut data = Vec::new();
            if self.bom {
                data.extend_from_slice(&[0xef, 0xbb, 0xbf]);
            }
            data.extend_from_slice(formatted.as_bytes());
            atomic_write(path, &data)?;
            self.path = Some(path.into());
            self.saved = formatted.clone();
        }
        self.text = formatted;
        self.schema = schema;
        self.error = None;
        self.save_layout(path, positions, !same)
    }
    pub fn save_layout(
        &mut self,
        path: &Path,
        positions: &Positions,
        force: bool,
    ) -> Result<(), String> {
        if force || self.layout_pending || positions != &self.saved_positions {
            self.layout_pending = true;
            let bytes = serde_json::to_vec_pretty(positions).map_err(|e| e.to_string())?;
            atomic_write(&sidecar(path), &bytes)
                .map_err(|e| format!("図の配置を保存できませんでした。{e}"))?;
            self.saved_positions = positions.clone();
            self.layout_pending = false;
        }
        Ok(())
    }
}
fn sidecar(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".layout.json");
    name.into()
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_diagram_history_and_disk_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shop.dbml");
        let original="\u{feff}// unchanged\r\nTable customers { id int [pk] }\r\nTable orders { cid int }\r\nTable members { id int }\r\nRef: orders.cid > customers.id\r\n";
        fs::write(&path, original).unwrap();
        let (mut doc, mut positions, _) = Document::open(&path).unwrap();
        let mut relation = doc.schema.refs[0].clone();
        relation.right.0 = "members".into();
        doc.apply(&Action::UpdateRef(0, relation.clone()), &mut positions)
            .unwrap();
        let expected = format!(
            "\u{feff}{}",
            crate::format::formatted(
                &original
                    .trim_start_matches('\u{feff}')
                    .replace("> customers.id", "> members.id")
            )
            .unwrap()
        );
        positions.insert("members".into(), [100., 80.]);
        doc.save(&path, &positions).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        let (mut doc, mut positions, _) = Document::open(&path).unwrap();
        assert_eq!(doc.schema.refs[0], relation);
        assert_eq!(positions["members"], [100., 80.]);
        doc.apply(&Action::DeleteColumn(2, 0), &mut positions)
            .unwrap();
        assert!(doc.schema.refs.is_empty());
        assert_eq!(doc.schema.tables[1].columns[0].name, "cid");
        doc.history(&mut positions, false);
        assert_eq!(doc.schema.refs[0], relation);
        assert_eq!(doc.text, expected.trim_start_matches('\u{feff}'));
        doc.text.push_str("Table unfinished {");
        doc.parse();
        assert!(doc.error.is_some());
        assert_eq!(doc.schema.refs[0], relation);
        assert!(doc.apply(&Action::DeleteTable(0), &mut positions).is_err());
    }
    #[test]
    fn save_as_retries_layout_even_when_coordinates_equal_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.dbml");
        let second = dir.path().join("b.dbml");
        let mut doc = Document::new();
        let layout = Positions::from([("orders".into(), [11., 99.])]);
        doc.save(&first, &layout).unwrap();
        fs::create_dir(sidecar(&second)).unwrap();
        assert!(doc.save(&second, &layout).is_err());
        assert!(doc.layout_pending);
        fs::remove_dir(sidecar(&second)).unwrap();
        doc.save(&second, &layout).unwrap();
        assert_eq!(Document::open(&second).unwrap().1, layout);
        fs::write(&second, "// external edit").unwrap();
        assert!(doc.save(&second, &layout).is_err());
        assert_eq!(fs::read_to_string(&second).unwrap(), "// external edit");
    }
}
