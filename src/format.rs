pub use crate::model::lint;
use crate::model::{quoted_or_comment_end, ref_start, Schema};
use dbml_rs::ast::TopLevelBlock;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Boundaries {
    declarations: HashSet<usize>,
    members: HashSet<usize>,
    types: HashMap<usize, usize>,
}

impl Boundaries {
    // The grammar allows several members on the same line. Only the AST can
    // distinguish, for example, two new columns from words inside a quoted type.
    fn from_schema(schema: &Schema) -> Result<Self, String> {
        let syntax = dbml_rs::parse_dbml_unchecked(&schema.masked).map_err(|e| e.to_string())?;
        let mut starts = Self::default();
        for block in syntax.blocks {
            let start = match block {
                TopLevelBlock::Project(p) => {
                    starts
                        .members
                        .extend(p.properties.iter().map(|v| v.span_range.start));
                    starts
                        .members
                        .extend(p.note.iter().map(|n| n.span_range.start));
                    p.span_range.start
                }
                TopLevelBlock::Table(t) => {
                    for col in &t.cols {
                        starts.members.insert(col.name.span_range.start);
                        starts
                            .types
                            .insert(col.r#type.span_range.start, col.r#type.span_range.end);
                    }
                    if let Some(ix) = &t.indexes {
                        // The parser's index-block span starts at its opening brace.
                        let end = schema.masked[..ix.span_range.start].trim_end().len();
                        starts.members.insert(end - "indexes".len());
                        starts
                            .members
                            .extend(ix.defs.iter().map(|d| d.span_range.start));
                    }
                    starts
                        .members
                        .extend(t.note.iter().map(|n| n.span_range.start));
                    t.span_range.start
                }
                TopLevelBlock::TableGroup(g) => {
                    starts
                        .members
                        .extend(g.items.iter().map(|v| v.span_range.start));
                    starts
                        .members
                        .extend(g.note.iter().map(|n| n.span_range.start));
                    g.span_range.start
                }
                TopLevelBlock::Enum(e) => {
                    starts
                        .members
                        .extend(e.values.iter().map(|v| v.value.span_range.start));
                    e.span_range.start
                }
                TopLevelBlock::Ref(r) => ref_start(&schema.masked, &r)?,
                TopLevelBlock::Note(n) => n.span_range.start,
            };
            starts.declarations.insert(start);
        }
        Ok(starts)
    }
}

fn punctuation(b: u8) -> bool {
    matches!(
        b,
        b'{' | b'}' | b'[' | b']' | b'(' | b')' | b'.' | b',' | b':' | b'<' | b'>' | b'-'
    )
}

fn token_end(source: &str, start: usize, hints: &Boundaries) -> Result<usize, String> {
    if let Some(end) = hints.types.get(&start) {
        return Ok(*end);
    }
    if let Some(end) = quoted_or_comment_end(source, start)? {
        return Ok(end);
    }
    let bytes = source.as_bytes();
    if bytes[start..].starts_with(b"<>") {
        return Ok(start + 2);
    }
    let signed = bytes[start] == b'-' && bytes.get(start + 1).is_some_and(u8::is_ascii_digit);
    if punctuation(bytes[start]) && !signed {
        return Ok(start + 1);
    }
    let mut end = start + 1;
    while end < bytes.len() {
        let b = bytes[end];
        if b.is_ascii_whitespace()
            || punctuation(b)
            || matches!(b, b'\'' | b'"' | b'`')
            || bytes[end..].starts_with(b"//")
            || bytes[end..].starts_with(b"/*")
        {
            break;
        }
        end += 1;
    }
    Ok(end)
}

pub fn formatted(source: &str) -> Result<String, String> {
    let schema = Schema::parse(source)?;
    lint(&schema, source)?;
    let hints = Boundaries::from_schema(&schema)?;
    let nl = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::with_capacity(source.len());
    let mut prev = "";
    let mut depth = 0usize;
    let mut end = 0;
    let bytes = source.as_bytes();
    while end < source.len() {
        let mut start = end;
        while start < bytes.len() && bytes[start].is_ascii_whitespace() {
            start += 1;
        }
        if start == bytes.len() {
            break;
        }
        let gap = &source[end..start];
        end = token_end(source, start, &hints)?;
        let token = &source[start..end];
        let comment = token.starts_with("//") || token.starts_with("/*");
        let prev_comment = prev.starts_with("//") || prev.starts_with("/*");
        if token == "}" {
            depth = depth.saturating_sub(1);
        }
        if !prev.is_empty() {
            let blank = gap.bytes().filter(|&b| b == b'\n').count() >= 2;
            let top = hints.declarations.contains(&start);
            let member = hints.members.contains(&start);
            let new_line = top
                || member
                || (prev == "{" && token != "}")
                || (token == "}" && prev != "{")
                || prev.starts_with("//")
                || ((comment || prev_comment) && gap.contains('\n'));
            if new_line {
                out.push_str(nl);
                if blank || (top && !prev_comment) {
                    out.push_str(nl);
                }
                for _ in 0..depth {
                    out.push_str("  ");
                }
            } else if !matches!(token, ":" | "," | "." | ")" | "]" | "}")
                && !matches!(prev, "." | "(" | "[")
            {
                out.push(' ');
            }
        }
        out.push_str(token);
        if token == "{" {
            depth += 1;
        }
        prev = token;
    }
    if !out.is_empty() {
        out.push_str(nl);
    }
    if out != source {
        let checked = Schema::parse(&out)?;
        lint(&checked, &out)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_compact_documents_idempotently() {
        let input="Table customers{id int[pk] name varchar(20)} Table orders {id int customer int[ref:>customers.id]}";
        let expected="Table customers {\n  id int [pk]\n  name varchar(20)\n}\n\nTable orders {\n  id int\n  customer int [ref: > customers.id]\n}\n";
        assert_eq!(formatted(input).unwrap(), expected);
        assert_eq!(formatted(expected).unwrap(), expected);
    }

    #[test]
    fn preserves_comments_strings_and_database_specific_types() {
        let block = "/* keep {\r\n      } spaces   */";
        let note = "'''\r\n  A { word } // not a comment\r\n\r\n   終わり  \r\n'''";
        let input=format!("// heading\r\nTable t{{\r\n col customtype [default: `x  +   y`, note: {note}] // end  \r\n{block}\r\n column \"double precision\"\r\n}}");
        let out = formatted(&input).unwrap();
        assert!(out.contains(note));
        assert!(out.contains(block));
        assert!(out.contains("// end  \r\n"));
        assert!(out.contains("`x  +   y`"));
        assert!(out.contains("\"double precision\""));
        assert!(!out.replace("\r\n", "").contains('\n'));
        assert_eq!(formatted(&out).unwrap(), out);
    }

    #[test]
    fn repeated_checks_signed_defaults_and_escaped_literals_survive() {
        let input = r#"Table t { n decimal [default:-12.50,check:`n>-20`,check:`n<0`] name text [default:'can\'t // split',note:'two slashes \\ and \'quotes\''] }"#;
        let out = formatted(input).unwrap();
        assert!(out.contains("default: -12.50, check: `n>-20`, check: `n<0`"));
        assert!(
            out.contains(r#"default: 'can\'t // split', note: 'two slashes \\ and \'quotes\''"#)
        );
        let schema = Schema::parse(&out).unwrap();
        let mut col = schema.tables[0].columns[0].clone();
        assert_eq!(col.default, "-12.50");
        col.default = "-9223372036854775808".into();
        let edited = schema
            .edit(&out, &crate::model::Action::UpdateColumn(0, 0, col))
            .unwrap();
        assert!(formatted(&edited)
            .unwrap()
            .contains("default: -9223372036854775808"));
        assert_eq!(formatted(&out).unwrap(), out);
        assert!(formatted(r#"Table t { n text [note:'invalid\'] }"#).is_err());
    }

    #[test]
    fn formats_advanced_blocks_and_references() {
        let input="Enum state{fresh old} Table a{id int code int indexes{(id,code)[pk]}} Table b{id int code int} Ref{a.(id,code)<>b.(id,code)}";
        let out = formatted(input).unwrap();
        assert!(out.contains("Enum state {\n  fresh\n  old\n}"));
        assert!(out.contains("indexes {\n    (id, code) [pk]\n  }"));
        assert!(out.contains("Ref {\n  a.(id, code) <> b.(id, code)\n}"));
        assert_eq!(formatted(&out).unwrap(), out);
    }

    #[test]
    fn live_and_save_reject_identical_semantic_errors() {
        for (input, diagnostic) in [
            ("Table users {\n  id int [null, not null]\n}", "2 行"),
            ("Table a { id int [pk,primary key] }", "重複"),
            ("Table a { id int indexes { missing } }", "missing"),
            (
                "Table a { id int } Table b { id int } Ref: a.(id,id) > b.id",
                "カラム数",
            ),
            (
                "Table a { id int } Table b { id int } Ref: a.(id,id) > b.(id,missing)",
                "missing",
            ),
            (
                "Table a { id int } Table b { aid int [ref: > a.id] } Ref: b.aid > a.id",
                "参照が重複",
            ),
        ] {
            let live = lint(&Schema::parse(input).unwrap(), input).unwrap_err();
            assert!(live.contains(diagnostic), "{live}");
            assert_eq!(formatted(input).unwrap_err(), live);
        }
        assert!(formatted("Table open {").is_err());
        assert_eq!(formatted("  ").unwrap(), "");
    }
}
