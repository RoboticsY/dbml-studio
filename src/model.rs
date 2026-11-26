mod keys;
use dbml_rs::ast as db;
pub(crate) use keys::ColumnKeys;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};
type Span = Range<usize>;
type Endpoint = (String, String);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: String,
    pub pk: bool,
    pub required: bool,
    pub unique: bool,
    pub default: String,
}
#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub start: usize,
    raw: db::TableBlock,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relation {
    pub left: Endpoint,
    pub right: Endpoint,
    pub kind: String,
}
#[derive(Clone, Debug)]
pub enum Action {
    AddTable(String),
    RenameTable(usize, String),
    DeleteTable(usize),
    AddColumn(usize, Column),
    UpdateColumn(usize, usize, Column),
    DeleteColumn(usize, usize),
    MoveColumn(usize, usize, usize),
    AddRef(Relation),
    UpdateRef(usize, Relation),
    DeleteRef(usize),
}
#[derive(Clone, Debug)]
enum RefSite {
    Block {
        whole: Span,
        left: db::RefIdent,
        right: db::RefIdent,
    },
    Inline {
        table: usize,
        col: usize,
        attr: Span,
        right: db::RefIdent,
    },
}
#[derive(Clone, Debug, Default)]
pub struct Schema {
    pub tables: Vec<Table>,
    pub refs: Vec<Relation>,
    sites: Vec<RefSite>,
    groups: Vec<db::TableGroupItem>,
    composite: Vec<db::RefBlock>,
    names: HashMap<String, usize>,
    pub(crate) masked: String,
    source: String,
    keys: std::sync::OnceLock<Vec<Vec<ColumnKeys>>>,
}
fn at(source: &str, offset: usize, message: &str) -> String {
    let prefix = &source[..offset];
    format!(
        "{} 行 {} 列  {message}",
        prefix.bytes().filter(|&c| c == b'\n').count() + 1,
        prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1
    )
}
// Shared lexical boundary for parser compatibility and lossless formatting.
pub(crate) fn quoted_or_comment_end(source: &str, i: usize) -> Result<Option<usize>, String> {
    let bytes = source.as_bytes();
    let rest = &bytes[i..];
    if rest.starts_with(b"//") {
        return Ok(Some(
            i + rest
                .iter()
                .position(|b| matches!(b, b'\r' | b'\n'))
                .unwrap_or(rest.len()),
        ));
    }
    if rest.starts_with(b"/*") {
        return rest[2..]
            .windows(2)
            .position(|w| w == b"*/")
            .map(|n| Some(i + 4 + n))
            .ok_or_else(|| at(source, i, "コメントを */ で閉じてください。"));
    }
    if !matches!(bytes[i], b'\'' | b'"' | b'`') {
        return Ok(None);
    }
    let n = if rest.starts_with(b"'''") { 3 } else { 1 };
    let mut end = i + n;
    while end < bytes.len() && !bytes[end..].starts_with(&rest[..n]) {
        if bytes[end] == b'\\' && end + 1 < bytes.len() {
            end += 1;
        }
        end += 1;
    }
    if end == bytes.len() {
        return Err(at(source, i, "引用符を閉じてください。"));
    }
    Ok(Some(end + n))
}
// dbml-rs 1.0.1 lacks escaped literal delimiters and signed numeric values.
// Change only its input view. All user-visible literal values use original byte spans.
fn parser_input(source: &str) -> Result<String, String> {
    let mut out = source.as_bytes().to_vec();
    let mut i = 0;
    let mut previous = None;
    while i < out.len() {
        if let Some(end) = quoted_or_comment_end(source, i)? {
            if out[i] == b'/' {
                for byte in &mut out[i..end] {
                    if !matches!(byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }
            } else {
                let delimiter = out[i];
                if delimiter != b'"' || previous == Some(b':') {
                    let mut pos = i + 1;
                    while pos + 1 < end {
                        if out[pos] == b'\\' {
                            pos += 1;
                            if out[pos] == delimiter {
                                out[pos] = b'_';
                            }
                        }
                        pos += 1;
                    }
                }
                previous = Some(delimiter);
            }
            i = end;
        } else {
            if out[i] == b'-'
                && previous == Some(b':')
                && out.get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                out[i] = b'0';
                out[i + 1] = b'0'; // also represent i64::MIN within the parser's signed range
            }
            if !out[i].is_ascii_whitespace() {
                previous = Some(out[i]);
            }
            i += 1;
        }
    }
    Ok(String::from_utf8(out).expect("mask is UTF-8"))
}
fn ident(name: &str) -> Result<String, String> {
    if name.is_empty() || name.contains(['"', '\n', '\r', '\0']) {
        return Err("名前を入力してください。二重引用符と改行は使えません。".into());
    }
    if name.as_bytes()[0].is_ascii_alphabetic()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        Ok(name.into())
    } else {
        Ok(format!("\"{name}\""))
    }
}
fn path(schema: Option<&db::Ident>, name: &db::Ident) -> String {
    let n = ident(&name.to_string).unwrap_or_else(|_| name.raw.clone());
    schema.map_or_else(
        || n.clone(),
        |s| {
            format!(
                "{}.{n}",
                ident(&s.to_string).unwrap_or_else(|_| s.raw.clone())
            )
        },
    )
}
fn table_input(input: &str) -> Result<String, String> {
    let s = input.trim();
    if !s.contains(['.', '"']) {
        return ident(s);
    }
    let probe = format!("Table {s} {{}}");
    let parsed = dbml_rs::parse_dbml_unchecked(&probe)
        .map_err(|_| "テーブル名は table または schema.table の形で入力してください。")?;
    let tables = parsed.tables();
    if parsed.blocks.len() != 1
        || tables.len() != 1
        || &probe[tables[0].ident.span_range.clone()] != s
    {
        return Err("テーブル名を確認してください。".into());
    }
    Ok(path(tables[0].ident.schema.as_ref(), &tables[0].ident.name))
}
pub(crate) fn ref_start(masked: &str, r: &db::RefBlock) -> Result<usize, String> {
    let first = r
        .name
        .as_ref()
        .map_or(r.lhs.span_range.start, |n| n.span_range.start);
    masked[..first]
        .trim_end_matches(|c: char| c.is_whitespace() || c == ':' || c == '{')
        .len()
        .checked_sub(3)
        .filter(|&p| {
            masked
                .get(p..p + 3)
                .is_some_and(|v| v.eq_ignore_ascii_case("ref"))
        })
        .ok_or_else(|| "Ref の開始位置を確認できません。".into())
}
fn symbol(r: &db::Relation) -> &'static str {
    match r {
        db::Relation::One2One => "-",
        db::Relation::One2Many => "<",
        db::Relation::Many2One => ">",
        db::Relation::Many2Many => "<>",
        db::Relation::Undef => "",
    }
}
fn mentions(sql: &str, name: &str) -> bool {
    sql.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word.eq_ignore_ascii_case(name))
        || sql.contains(&format!("\"{name}\""))
}
fn end_col(c: &db::TableColumn) -> usize {
    c.settings
        .as_ref()
        .map_or(c.r#type.span_range.end, |s| s.span_range.end)
}
fn col_span(c: &db::TableColumn) -> Span {
    c.name.span_range.start..end_col(c)
}
fn projected(c: &db::TableColumn, source: &str) -> Column {
    let s = c.settings.as_ref();
    Column {
        name: c.name.to_string.clone(),
        ty: c.r#type.raw.clone(),
        pk: s.is_some_and(|s| s.is_pk),
        required: s.is_some_and(|s| s.nullable == Some(db::Nullable::NotNull)),
        unique: s.is_some_and(|s| s.is_unique),
        default: s
            .and_then(|s| s.attributes.iter().find(|a| a.key.to_string == "default"))
            .and_then(|a| a.value.as_ref())
            .map(|v| source[v.span_range.clone()].to_owned())
            .unwrap_or_default(),
    }
}
impl Schema {
    pub fn parse(source: &str) -> Result<Self, String> {
        let masked = parser_input(source)?;
        let ast = dbml_rs::parse_dbml_unchecked(&masked).map_err(|e| e.to_string())?;
        let mut model = Self::default();
        for raw in ast.tables() {
            let name = path(raw.ident.schema.as_ref(), &raw.ident.name);
            let cols: Vec<_> = raw.cols.iter().map(|c| projected(c, source)).collect();
            let mut seen = HashSet::new();
            for (i, col) in cols.iter().enumerate() {
                if !seen.insert(&col.name) {
                    return Err(at(
                        source,
                        raw.cols[i].name.span_range.start,
                        "カラム名が重複しています。",
                    ));
                }
            }
            let mut names = vec![name.clone()];
            if raw.ident.schema.is_none() {
                names.push(format!("public.{name}"));
            }
            if let Some(n) = name.strip_prefix("public.") {
                names.push(n.into());
            }
            if let Some(alias) = &raw.ident.alias {
                names.push(path(None, alias));
            }
            for key in names {
                if model
                    .names
                    .insert(key.clone(), model.tables.len())
                    .is_some_and(|i| i != model.tables.len())
                {
                    return Err(at(
                        source,
                        raw.span_range.start,
                        &format!("テーブル名または別名 {key} が重複しています。"),
                    ));
                }
            }
            model.tables.push(Table {
                name,
                columns: cols,
                start: raw.span_range.start,
                raw: raw.clone(),
            });
        }
        for (i, t) in ast.tables().into_iter().enumerate() {
            for (j, c) in t.cols.iter().enumerate() {
                for r in c.settings.iter().flat_map(|s| &s.refs) {
                    let right = model.endpoint(&r.rhs, source)?;
                    model.refs.push(Relation {
                        left: (model.tables[i].name.clone(), c.name.to_string.clone()),
                        right,
                        kind: symbol(&r.rel).into(),
                    });
                    model.sites.push(RefSite::Inline {
                        table: i,
                        col: j,
                        attr: r.span_range.clone(),
                        right: r.rhs.clone(),
                    });
                }
            }
        }
        for r in ast.refs() {
            if r.lhs.compositions.len() != 1 || r.rhs.compositions.len() != 1 {
                model.composite.push(r.clone());
                continue;
            }
            let left = model.endpoint(&r.lhs, source)?;
            let right = model.endpoint(&r.rhs, source)?;
            let start = ref_start(&masked, r)?;
            let block = masked[start..r.lhs.span_range.start]
                .trim_end()
                .ends_with('{');
            let mut end = r
                .settings
                .as_ref()
                .map_or(r.rhs.span_range.end, |s| s.span_range.end);
            if block {
                end += masked[end..]
                    .find('}')
                    .ok_or("Ref の閉じ括弧がありません。")?
                    + 1;
            }
            model.refs.push(Relation {
                left,
                right,
                kind: symbol(&r.rel).into(),
            });
            model.sites.push(RefSite::Block {
                whole: start..end,
                left: r.lhs.clone(),
                right: r.rhs.clone(),
            });
        }
        model.groups = ast
            .table_groups()
            .iter()
            .flat_map(|g| g.items.iter().cloned())
            .collect();
        model.masked = masked;
        model.source = source.into();
        Ok(model)
    }
    fn endpoint(&self, id: &db::RefIdent, source: &str) -> Result<Endpoint, String> {
        let key = path(id.schema.as_ref(), &id.table);
        let table = self
            .names
            .get(&key)
            .map(|&i| &self.tables[i])
            .ok_or_else(|| {
                at(
                    source,
                    id.table.span_range.start,
                    &format!("テーブル {key} が見つかりません。"),
                )
            })?;
        let col = id
            .compositions
            .first()
            .filter(|_| id.compositions.len() == 1)
            .ok_or("複合参照はテキストで編集してください。")?;
        if !table.columns.iter().any(|c| c.name == col.to_string) {
            return Err(at(
                source,
                col.span_range.start,
                &format!("{key} にカラム {} がありません。", col.raw),
            ));
        }
        Ok((table.name.clone(), col.to_string.clone()))
    }
}

/// Check cross-definition constraints without imposing a database dialect.
pub fn lint(schema: &Schema, source: &str) -> Result<(), String> {
    if source != schema.source {
        return Err("テキストが変わりました。もう一度検査してください。".into());
    }
    for table in &schema.tables {
        for col in &table.raw.cols {
            if let Some(settings) = &col.settings {
                let mut keys = HashSet::new();
                for attr in &settings.attributes {
                    let key = match attr.key.to_string.as_str() {
                        "primary key" => "pk",
                        key @ ("pk" | "unique" | "null" | "not null" | "default" | "increment"
                        | "note") => key,
                        _ => continue,
                    };
                    if !keys.insert(key) {
                        return Err(at(
                            source,
                            attr.span_range.start,
                            &format!("設定 {key} が重複しています。"),
                        ));
                    }
                }
                if keys.contains("null") && keys.contains("not null") {
                    return Err(at(
                        source,
                        settings.span_range.start,
                        "null と not null のどちらかを指定してください。",
                    ));
                }
            }
        }
        for index in table.raw.indexes.iter().flat_map(|block| &block.defs) {
            for field in &index.cols {
                if let db::IndexesColumnType::String(id) = field {
                    if !table.columns.iter().any(|c| c.name == id.to_string) {
                        return Err(at(
                            source,
                            id.span_range.start,
                            &format!(
                                "{} にインデックスの対象カラム {} がありません。",
                                table.name, id.raw
                            ),
                        ));
                    }
                }
            }
        }
    }
    let mut pairs = HashSet::new();
    for (relation, site) in schema.refs.iter().zip(&schema.sites) {
        let (left, right) = (&relation.left, &relation.right);
        if !pairs.insert((left.min(right), left.max(right))) {
            let pos = match site {
                RefSite::Block { whole, .. } => whole.start,
                RefSite::Inline { attr, .. } => attr.start,
            };
            return Err(at(source, pos, "同じカラム間の参照が重複しています。"));
        }
    }
    for r in &schema.composite {
        if r.lhs.compositions.len() != r.rhs.compositions.len() {
            return Err(at(
                source,
                r.span_range.start,
                "複合参照の両端でカラム数をそろえてください。",
            ));
        }
        for id in [&r.lhs, &r.rhs] {
            let name = path(id.schema.as_ref(), &id.table);
            let t = schema.names.get(&name).ok_or_else(|| {
                at(
                    source,
                    id.table.span_range.start,
                    &format!("テーブル {name} が見つかりません。"),
                )
            })?;
            for c in &id.compositions {
                if !schema.tables[*t]
                    .columns
                    .iter()
                    .any(|field| field.name == c.to_string)
                {
                    return Err(at(
                        source,
                        c.span_range.start,
                        &format!("{name} にカラム {} がありません。", c.raw),
                    ));
                }
            }
        }
    }
    Ok(())
}

type SettingChanges = Vec<(Option<Span>, String)>;
#[derive(Default)]
struct Edits {
    changes: Vec<(Span, String)>,
    fields: std::collections::BTreeMap<(usize, usize), SettingChanges>,
}
impl Edits {
    fn put(&mut self, span: Span, text: impl Into<String>) {
        self.changes.push((span, text.into()));
    }
    fn field(&mut self, t: usize, c: usize, span: Option<Span>, value: String) {
        self.fields.entry((t, c)).or_default().push((span, value));
    }
    fn finish(mut self, m: &Schema) -> Result<String, String> {
        for ((ti, ci), mods) in std::mem::take(&mut self.fields) {
            let col = &m.tables[ti].raw.cols[ci];
            let Some(settings) = &col.settings else {
                let vals = mods
                    .into_iter()
                    .map(|(_, v)| v)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.put(end_col(col)..end_col(col), format!(" [{vals}]"));
                continue;
            };
            let mut items = settings
                .attributes
                .iter()
                .map(|a| a.span_range.clone())
                .chain(settings.refs.iter().map(|r| r.span_range.clone()))
                .collect::<Vec<_>>();
            items.sort_by_key(|r| r.start);
            let deleted: HashSet<_> = mods
                .iter()
                .filter_map(|(s, v)| s.as_ref().filter(|_| v.is_empty()).map(|r| r.start))
                .collect();
            let added = mods
                .iter()
                .filter(|(s, _)| s.is_none())
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let remain: Vec<_> = items
                .iter()
                .filter(|r| !deleted.contains(&r.start))
                .collect();
            // Keep the comma immediately after each surviving item except the last.
            for pair in items.windows(2) {
                let lo = m.clean(&pair[0]).end;
                let hi = pair[1].start;
                if !deleted.contains(&pair[0].start)
                    && !remain.last().is_some_and(|r| r.start == pair[0].start)
                {
                    continue;
                }
                if let Some(n) = m.masked[lo..hi].find(',') {
                    self.put(lo + n..lo + n + 1, "");
                }
            }
            let close = settings.span_range.end - 1;
            if !added.is_empty() {
                self.put(
                    close..close,
                    format!("{}{added}", if remain.is_empty() { "" } else { ", " }),
                );
            }
            if remain.is_empty() && added.is_empty() {
                self.put(settings.span_range.start..settings.span_range.start + 1, "");
                self.put(close..close + 1, "");
            }
            for (s, v) in mods {
                if let Some(s) = s {
                    self.put(m.clean(&s), v);
                }
            }
        }
        let mut out = m.source.clone();
        self.changes.sort_by_key(|(r, _)| (r.start, r.end));
        let mut prev = out.len();
        for (r, v) in self.changes.into_iter().rev() {
            if r.end > prev || !out.is_char_boundary(r.start) || !out.is_char_boundary(r.end) {
                return Err("変更範囲が重なっています。テキストで編集してください。".into());
            }
            prev = r.start;
            out.replace_range(r, &v);
        }
        Ok(out)
    }
}
impl Schema {
    fn clean(&self, s: &Span) -> Span {
        s.start..s.start + self.masked[s.clone()].trim_end().len()
    }
    fn table(&self, t: usize) -> Result<&Table, String> {
        self.tables
            .get(t)
            .ok_or_else(|| "テーブルを選び直してください。".into())
    }
    fn column(&self, t: usize, c: usize) -> Result<&db::TableColumn, String> {
        self.table(t)?
            .raw
            .cols
            .get(c)
            .ok_or_else(|| "カラムを選び直してください。".into())
    }
    fn check_dependents(&self, t: usize, col: Option<&str>) -> Result<(), String> {
        let table = &self.tables[t];
        let mut blocked = None;
        if let (Some(name), Some(ix)) = (col, &table.raw.indexes) {
            blocked = ix
                .defs
                .iter()
                .find(|def| {
                    def.cols.iter().any(|c| match c {
                        db::IndexesColumnType::String(id) => id.to_string == name,
                        db::IndexesColumnType::Expr(expr) => mentions(&expr.raw, name),
                    })
                })
                .map(|d| d.span_range.clone());
        }
        if blocked.is_none() {
            blocked = self
                .composite
                .iter()
                .find(|r| {
                    [&r.lhs, &r.rhs].iter().any(|id| {
                        self.names.get(&path(id.schema.as_ref(), &id.table)) == Some(&t)
                            && col.is_none_or(|name| {
                                id.compositions.iter().any(|i| i.to_string == name)
                            })
                    })
                })
                .map(|r| r.span_range.clone());
        }
        if col.is_none() && blocked.is_none() {
            blocked = self
                .groups
                .iter()
                .find(|g| self.names.get(&path(g.schema.as_ref(), &g.ident_alias)) == Some(&t))
                .map(|g| g.span_range.clone());
        }
        if let Some(s) = blocked {
            return Err(at(
                &self.source,
                s.start,
                &format!(
                    "先にテキストで関連する定義を変更してください。\n{}",
                    self.source[s].trim()
                ),
            ));
        }
        Ok(())
    }
    fn remove_ref(&self, n: usize, e: &mut Edits, removed: Option<&Span>) -> Result<(), String> {
        match self.sites.get(n).ok_or("参照を選び直してください。")? {
            RefSite::Block { whole, .. } => e.put(whole.clone(), ""),
            RefSite::Inline {
                table, col, attr, ..
            } => {
                if !removed.is_some_and(|r| r.contains(&attr.start)) {
                    e.field(*table, *col, Some(attr.clone()), String::new());
                }
            }
        }
        Ok(())
    }
    fn validate_relation(&self, r: &Relation, skip: Option<usize>) -> Result<(), String> {
        for (t, c) in [&r.left, &r.right] {
            if !self
                .tables
                .iter()
                .any(|v| v.name == *t && v.columns.iter().any(|v| v.name == *c))
            {
                return Err(format!("参照先 {t}.{c} が見つかりません。"));
            }
        }
        if !matches!(r.kind.as_str(), "<" | ">" | "-" | "<>") {
            return Err("関係の種類を選んでください。".into());
        }
        if self.refs.iter().enumerate().any(|(i, v)| {
            Some(i) != skip
                && ((v.left == r.left && v.right == r.right)
                    || (v.left == r.right && v.right == r.left))
        }) {
            return Err("同じカラム間の参照があります。".into());
        }
        Ok(())
    }
    fn rewrite_names(
        &self,
        e: &mut Edits,
        t: usize,
        col: Option<&str>,
        new: &str,
    ) -> Result<(), String> {
        let table = &self.tables[t].raw;
        let rename_table = |e: &mut Edits, schema: Option<&db::Ident>, name: &db::Ident| {
            let key = path(schema, name);
            if self.names.get(&key) == Some(&t)
                && !table
                    .ident
                    .alias
                    .as_ref()
                    .is_some_and(|a| path(None, a) == key)
            {
                let start = schema.map_or(name.span_range.start, |s| s.span_range.start);
                e.put(start..name.span_range.end, new);
            }
        };
        let endpoints = self
            .sites
            .iter()
            .flat_map(|s| match s {
                RefSite::Block { left, right, .. } => [Some(left), Some(right)],
                RefSite::Inline { right, .. } => [None, Some(right)],
            })
            .flatten()
            .chain(self.composite.iter().flat_map(|r| [&r.lhs, &r.rhs]));
        for id in endpoints {
            if let Some(old) = col {
                if self.names.get(&path(id.schema.as_ref(), &id.table)) == Some(&t) {
                    for c in &id.compositions {
                        if c.to_string == old {
                            e.put(c.span_range.clone(), new);
                        }
                    }
                }
            } else {
                rename_table(e, id.schema.as_ref(), &id.table);
            }
        }
        if let Some(old) = col {
            for index in table.indexes.iter().flat_map(|ix| &ix.defs) {
                for c in &index.cols {
                    match c {
                        db::IndexesColumnType::String(id) if id.to_string == old => {
                            e.put(id.span_range.clone(), new)
                        }
                        db::IndexesColumnType::Expr(expr) if mentions(&expr.raw, old) => {
                            return Err(at(
                                &self.source,
                                index.span_range.start,
                                &format!("先にテキストで式を更新してください。\n{}", expr.raw),
                            ))
                        }
                        _ => {}
                    }
                }
            }
        } else {
            for item in &self.groups {
                rename_table(e, item.schema.as_ref(), &item.ident_alias);
            }
        }
        Ok(())
    }
    fn remove_affected(
        &self,
        t: usize,
        col: Option<&str>,
        span: &Span,
        e: &mut Edits,
    ) -> Result<(), String> {
        for (i, r) in self.refs.iter().enumerate() {
            if [&r.left, &r.right]
                .iter()
                .any(|(name, c)| name == &self.tables[t].name && col.is_none_or(|v| v == c))
            {
                self.remove_ref(i, e, Some(span))?;
            }
        }
        Ok(())
    }
    fn update_settings(&self, t: usize, c: usize, new: &Column, e: &mut Edits) {
        let old = &self.tables[t].columns[c];
        let raw = &self.tables[t].raw.cols[c];
        let flags = [
            (old.pk != new.pk, "pk", if new.pk { "pk" } else { "" }),
            (
                old.required != new.required,
                "not null",
                if new.required { "not null" } else { "" },
            ),
            (
                old.unique != new.unique,
                "unique",
                if new.unique { "unique" } else { "" },
            ),
            (old.default != new.default, "default", new.default.as_str()),
        ];
        for (changed, key, value) in flags {
            if !changed {
                continue;
            }
            let attr = raw.settings.iter().flat_map(|s| &s.attributes).find(|a| {
                let k = a.key.to_string.as_str();
                k == key
                    || (key == "pk" && k == "primary key")
                    || (key == "not null" && k == "null")
            });
            if key == "default" && !value.is_empty() {
                if let Some(lit) = attr.and_then(|a| a.value.as_ref()) {
                    e.put(lit.span_range.clone(), value);
                } else {
                    e.field(
                        t,
                        c,
                        attr.map(|a| a.span_range.clone()),
                        format!("default: {value}"),
                    );
                }
            } else {
                e.field(t, c, attr.map(|a| a.span_range.clone()), value.into());
            }
        }
    }
    pub fn edit(&self, source: &str, action: &Action) -> Result<String, String> {
        if source != self.source {
            return Err("テキストが変更されました。編集対象を選び直してください。".into());
        }
        let mut e = Edits::default();
        let nl = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let append = |e: &mut Edits, stmt: String| {
            let sep = if source.is_empty() || source.ends_with('\n') {
                ""
            } else {
                nl
            };
            e.put(source.len()..source.len(), format!("{sep}{stmt}{nl}"));
        };
        match action {
            Action::AddTable(name) => {
                append(&mut e, format!("Table {} {{{nl}}}", table_input(name)?))
            }
            Action::RenameTable(t, name) => {
                let old = self.table(*t)?;
                let new = table_input(name)?;
                if new != old.name {
                    e.put(old.raw.ident.span_range.clone(), &new);
                    self.rewrite_names(&mut e, *t, None, &new)?;
                }
            }
            Action::DeleteTable(t) => {
                let tab = self.table(*t)?;
                self.check_dependents(*t, None)?;
                self.remove_affected(*t, None, &tab.raw.span_range, &mut e)?;
                e.put(tab.raw.span_range.clone(), "");
            }
            Action::AddColumn(t, col) => {
                let table = self.table(*t)?;
                let text = column_text(col)?;
                let close = table.raw.span_range.end - 1;
                let line = source[..close].rfind('\n').map_or(0, |i| i + 1);
                let (pos, prefix) = if source[line..close].trim().is_empty() {
                    (line, "")
                } else {
                    (close, nl)
                };
                let indent = table
                    .raw
                    .cols
                    .first()
                    .map(|c| {
                        let p = c.name.span_range.start;
                        &source[source[..p].rfind('\n').map_or(0, |i| i + 1)..p]
                    })
                    .filter(|s| !s.is_empty() && s.trim().is_empty())
                    .unwrap_or("  ");
                e.put(pos..pos, format!("{prefix}{indent}{text}{nl}"));
            }
            Action::UpdateColumn(t, c, col) => {
                let raw = self.column(*t, *c)?;
                let old = &self.tables[*t].columns[*c];
                column_text(col)?;
                if old.name != col.name {
                    let name = ident(&col.name)?;
                    e.put(raw.name.span_range.clone(), &name);
                    self.rewrite_names(&mut e, *t, Some(&old.name), &name)?;
                }
                if old.ty != col.ty {
                    e.put(raw.r#type.span_range.clone(), &col.ty);
                }
                self.update_settings(*t, *c, col, &mut e);
            }
            Action::DeleteColumn(t, c) => {
                let raw = self.column(*t, *c)?;
                self.check_dependents(*t, Some(&raw.name.to_string))?;
                let span = col_span(raw);
                self.remove_affected(*t, Some(&raw.name.to_string), &span, &mut e)?;
                e.put(span, "");
            }
            Action::MoveColumn(t, from, to) => {
                self.column(*t, *from)?;
                self.column(*t, *to)?;
                let cols = &self.tables[*t].raw.cols;
                let (lo, hi) = ((*from).min(*to), (*from).max(*to));
                let spans = cols[lo..=hi]
                    .iter()
                    .map(|c| {
                        let mut span = col_span(c);
                        let end = source[span.end..]
                            .find(['\r', '\n'])
                            .map_or(source.len(), |i| span.end + i);
                        let tail = &source[span.end..end];
                        if tail.trim_start().starts_with("//") {
                            span.end = end;
                        }
                        span
                    })
                    .collect::<Vec<_>>();
                let mut order = spans.clone();
                if from < to {
                    order.rotate_left(1);
                } else {
                    order.rotate_right(1);
                }
                for (dest, src) in spans.into_iter().zip(order) {
                    e.put(dest, &source[src]);
                }
            }
            Action::DeleteRef(i) => self.remove_ref(*i, &mut e, None)?,
            Action::AddRef(r) | Action::UpdateRef(_, r) => {
                let idx = if let Action::UpdateRef(i, _) = action {
                    Some(*i)
                } else {
                    None
                };
                self.validate_relation(r, idx)?;
                let endpoint = |(t, c): &Endpoint| -> Result<String, String> {
                    Ok(format!("{t}.{}", ident(c)?))
                };
                let statement = format!(
                    "Ref: {} {} {}",
                    endpoint(&r.left)?,
                    r.kind,
                    endpoint(&r.right)?
                );
                if let Some(i) = idx {
                    let old = self.refs.get(i).ok_or("参照を選び直してください。")?;
                    match &self.sites[i] {
                        RefSite::Block { left, right, .. } => {
                            if old.left != r.left {
                                e.put(left.span_range.clone(), endpoint(&r.left)?);
                            }
                            if old.right != r.right {
                                e.put(right.span_range.clone(), endpoint(&r.right)?);
                            }
                            if old.kind != r.kind {
                                let start = left.span_range.end;
                                let between = &self.masked[start..right.span_range.start];
                                let op = between
                                    .find(&old.kind)
                                    .ok_or("関係の種類を確認できません。")?
                                    + start;
                                e.put(op..op + old.kind.len(), &r.kind);
                            }
                        }
                        RefSite::Inline { attr, right, .. } if old.left == r.left => {
                            if old.right != r.right {
                                e.put(right.span_range.clone(), endpoint(&r.right)?);
                            }
                            if old.kind != r.kind {
                                let start = attr.start;
                                let between = &self.masked[start..right.span_range.start];
                                let op = between
                                    .find(&old.kind)
                                    .ok_or("関係の種類を確認できません。")?
                                    + start;
                                e.put(op..op + old.kind.len(), &r.kind);
                            }
                        }
                        _ => {
                            self.remove_ref(i, &mut e, None)?;
                            append(&mut e, statement);
                        }
                    }
                } else {
                    append(&mut e, statement);
                }
            }
        }
        let result = e.finish(self)?;
        Self::parse(&result)?;
        Ok(result)
    }
}
fn column_text(c: &Column) -> Result<String, String> {
    let mut attrs = Vec::new();
    if c.pk {
        attrs.push("pk".into());
    }
    if c.required {
        attrs.push("not null".into());
    }
    if c.unique {
        attrs.push("unique".into());
    }
    if !c.default.is_empty() {
        attrs.push(format!("default: {}", c.default));
    }
    let mut text = format!("{} {}", ident(&c.name)?, c.ty);
    if !attrs.is_empty() {
        text.push_str(&format!(" [{}]", attrs.join(", ")));
    }
    // Validate the submitted fields in isolation so a pasted type/default cannot introduce extra DBML.
    let probe = format!("Table Probe {{ {text} }}");
    let masked = parser_input(&probe)?;
    let ast = dbml_rs::parse_dbml_unchecked(&masked).map_err(|e| e.to_string())?;
    let tables = ast.tables();
    if ast.blocks.len() != 1
        || tables.len() != 1
        || tables[0].cols.len() != 1
        || projected(&tables[0].cols[0], &probe) != *c
        || tables[0].cols[0]
            .settings
            .as_ref()
            .is_some_and(|s| s.attributes.len() != attrs.len() || !s.refs.is_empty())
    {
        return Err("カラム名、型、既定値を確認してください。".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(s: &str, a: Action) -> String {
        Schema::parse(s).unwrap().edit(s, &a).unwrap()
    }
    const S:&str="// 説明\r\nTable customers as C {\r\n  id int [pk] // primary\r\n  code varchar(16)\r\n}\r\n\r\nTable orders {\r\n  id int [pk]\r\n  customer int [ref: > C.id, note: 'a,b']\r\n}\r\n";
    #[test]
    fn edit_preserves_source_and_updates_references() {
        let m = Schema::parse(S).unwrap();
        let mut col = m.tables[0].columns[0].clone();
        col.name = "customer_id".into();
        let renamed = edit(S, Action::UpdateColumn(0, 0, col));
        assert_eq!(
            renamed,
            S.replacen("id int", "customer_id int", 1)
                .replace("C.id", "C.customer_id")
        );
        let deleted = edit(&renamed, Action::DeleteColumn(0, 0));
        let m = Schema::parse(&deleted).unwrap();
        assert!(m.refs.is_empty());
        assert_eq!(m.tables[1].columns[1].name, "customer");
        assert!(deleted.contains("note: 'a,b'"));
        assert!(deleted.contains("// primary"));
    }
    #[test]
    fn standalone_named_short_block_and_inline_relations() {
        for declaration in [
            "Ref named { orders.customer > customers.id [delete: cascade] }",
            "Ref named: orders.customer > customers.id [delete: cascade]",
            "Ref: orders.customer > customers.id",
            "Ref { orders.customer > customers.id }",
        ] {
            let s = S.replace("ref: > C.id, ", "") + declaration + "\r\n// end";
            let m = Schema::parse(&s).unwrap();
            let mut r = m.refs[0].clone();
            r.right.1 = "code".into();
            r.kind = "-".into();
            let changed = edit(&s, Action::UpdateRef(0, r));
            assert_eq!(
                changed,
                s.replace("customer > customers.id", "customer - customers.code")
            );
            let deleted = edit(&s, Action::DeleteRef(0));
            assert!(deleted.ends_with("\r\n// end"));
            assert!(Schema::parse(&deleted).unwrap().refs.is_empty());
        }
    }
    #[test]
    fn composite_and_index_guard() {
        let s="Enum state { open closed }\nTable a { id int\ncode int\n status state\n indexes { (id, code) [pk] } }\nTable b { id int code int }\nRef: a.(id,code) - b.(id,code)";
        let m = Schema::parse(s).unwrap();
        assert!(m
            .edit(s, &Action::DeleteColumn(0, 0))
            .unwrap_err()
            .contains("(id, code)"));
        assert!(m
            .edit(s, &Action::DeleteColumn(1, 0))
            .unwrap_err()
            .contains("a.(id,code)"));
        let mut col = m.tables[0].columns[0].clone();
        col.name = "key".into();
        let renamed = edit(s, Action::UpdateColumn(0, 0, col));
        assert!(renamed.contains("(key, code) [pk]"));
        assert!(renamed.contains("a.(key,code) - b.(id,code)"));
        let out = edit(s, Action::AddTable("sales.orders".into()));
        assert!(out.starts_with(s));
    }
    #[test]
    fn move_column_keeps_its_trailing_comment() {
        let source = "Table users {\r\n  a int // first\r\n  b int // second\r\n}";
        assert_eq!(
            edit(source, Action::MoveColumn(0, 0, 1)),
            "Table users {\r\n  b int // second\r\n  a int // first\r\n}"
        );
    }
    #[test]
    fn column_attributes_and_ordering() {
        let m = Schema::parse(S).unwrap();
        let mut c = m.tables[1].columns[1].clone();
        c.pk = true;
        c.default = "'¥10'".into();
        c.required = true;
        c.unique = true;
        let s = edit(S, Action::UpdateColumn(1, 1, c.clone()));
        assert_eq!(Schema::parse(&s).unwrap().tables[1].columns[1], c);
        c.pk = false;
        c.unique = false;
        c.default.clear();
        c.required = false;
        let s = edit(&s, Action::UpdateColumn(1, 1, c));
        let s = edit(&s, Action::MoveColumn(1, 1, 0));
        let m = Schema::parse(&s).unwrap();
        assert_eq!(m.tables[1].columns[0].name, "customer");
        assert_eq!(m.refs[0].left.1, "customer");
        assert!(s.contains("note: 'a,b'"));
    }
    #[test]
    fn add_columns_and_refs_in_new_schema() {
        let mut s = edit("", Action::AddTable("人".into()));
        s = edit(
            &s,
            Action::AddColumn(
                0,
                Column {
                    name: "番号".into(),
                    ty: "integer".into(),
                    pk: true,
                    ..Column::default()
                },
            ),
        );
        s = edit(&s, Action::AddTable("sales.orders".into()));
        s = edit(
            &s,
            Action::AddColumn(
                1,
                Column {
                    name: "person".into(),
                    ty: "integer".into(),
                    ..Column::default()
                },
            ),
        );
        let r = Relation {
            left: ("sales.orders".into(), "person".into()),
            right: ("\"人\"".into(), "番号".into()),
            kind: ">".into(),
        };
        s = edit(&s, Action::AddRef(r.clone()));
        assert_eq!(Schema::parse(&s).unwrap().refs[0], r);
        s = edit(&s, Action::RenameTable(1, "sales.invoices".into()));
        assert!(s.contains("Ref: sales.invoices.person"));
        s = edit(&s, Action::DeleteTable(0));
        assert_eq!(Schema::parse(&s).unwrap().tables.len(), 1);
    }
    #[test]
    fn malformed_text_and_bad_form_fields() {
        assert!(Schema::parse("Table ok {}\nTable bad {\n  x\n}").is_err());
        let m = Schema::parse(S).unwrap();
        assert!(m
            .edit(
                S,
                &Action::AddColumn(
                    0,
                    Column {
                        name: "x".into(),
                        ty: "int\ny int".into(),
                        ..Column::default()
                    }
                )
            )
            .is_err());
        assert!(m.edit(S, &Action::AddTable("customers".into())).is_err());
    }
}
