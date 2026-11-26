use super::{db, path, symbol, RefSite, Schema};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct ColumnKeys {
    pub primary: bool,
    pub foreign: bool,
}

// In short/long 1:1 refs DBML puts the FK second; inline 1:1 puts it on the declaring column.
// A direct many-to-many ref represents a junction table, with no physical FK on either endpoint.
fn foreign_endpoint<T>(relation: &str, inline: bool, left: T, right: T) -> Option<T> {
    match relation {
        ">" => Some(left),
        "-" if inline => Some(left),
        "<" | "-" => Some(right),
        _ => None,
    }
}

impl Schema {
    pub(crate) fn column_keys(&self) -> &[Vec<ColumnKeys>] {
        self.keys.get_or_init(|| {
            let mut keys: Vec<Vec<_>> = self
                .tables
                .iter()
                .map(|table| {
                    let mut row: Vec<_> = table
                        .columns
                        .iter()
                        .map(|c| ColumnKeys {
                            primary: c.pk,
                            foreign: false,
                        })
                        .collect();
                    for index in table
                        .raw
                        .indexes
                        .iter()
                        .flat_map(|ix| &ix.defs)
                        .filter(|ix| ix.settings.as_ref().is_some_and(|s| s.is_pk))
                    {
                        for col in &index.cols {
                            if let db::IndexesColumnType::String(name) = col {
                                if let Some(i) =
                                    table.columns.iter().position(|c| c.name == name.to_string)
                                {
                                    row[i].primary = true;
                                }
                            }
                        }
                    }
                    row
                })
                .collect();
            let mut mark_foreign = |table: &str, column: &str| {
                if let Some(&t) = self.names.get(table) {
                    if let Some(c) = self.tables[t].columns.iter().position(|c| c.name == column) {
                        keys[t][c].foreign = true;
                    }
                }
            };
            for (r, site) in self.refs.iter().zip(&self.sites) {
                if let Some((table, col)) = foreign_endpoint(
                    &r.kind,
                    matches!(site, RefSite::Inline { .. }),
                    &r.left,
                    &r.right,
                ) {
                    mark_foreign(table, col);
                }
            }
            for r in &self.composite {
                if let Some(end) = foreign_endpoint(symbol(&r.rel), false, &r.lhs, &r.rhs) {
                    let table = path(end.schema.as_ref(), &end.table);
                    for c in &end.compositions {
                        mark_foreign(&table, &c.to_string);
                    }
                }
            }
            keys
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_and_foreign_roles_follow_the_declaration() {
        let schema = Schema::parse(
            r#"
Table core.parent as p {
    id int
    code int
    independent int
    indexes { (id, code) [pk] }
}
Table child {
    owner int [pk, ref: - p.id]
    by_right int
    by_left int
    extra int
    composite_id int
    composite_code int
    logical_many int
}
Ref: p.code - child.by_right
Ref: child.by_left > p.id
Ref: p.id < child.extra
Ref: child.(composite_id, composite_code) > p.(id, code)
Ref: p.independent <> child.logical_many
"#,
        )
        .unwrap();
        let keys = schema.column_keys();
        let roles = |t: usize| {
            keys[t]
                .iter()
                .map(|k| (k.primary, k.foreign))
                .collect::<Vec<_>>()
        };
        assert_eq!(roles(0), [(true, false), (true, false), (false, false)]);
        assert_eq!(
            roles(1),
            [
                (true, true),
                (false, true),
                (false, true),
                (false, true),
                (false, true),
                (false, true),
                (false, false)
            ]
        );
    }
}
