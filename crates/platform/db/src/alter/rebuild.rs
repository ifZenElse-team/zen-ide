use super::{Note, Reason, Refusal, Writer};
use crate::schema::{ConstraintKind, Object};

const LOST: [&str; 8] = ["CHECK", "GENERATED", "AS", "AUTOINCREMENT", "COLLATE", "STRICT", "WITHOUT", "DEFERRABLE"];

const NOT_RESTATED: [&str; 5] = ["ON UPDATE", "INVISIBLE", "SRID", "COLUMN_FORMAT", "STORAGE"];

const TEMPORARY: &str = "_zen_new_";

pub(super) fn words(sql: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' | '[' => {
                let close = if c == '[' { ']' } else { c };
                for inner in chars.by_ref() {
                    if inner == close {
                        break;
                    }
                }
                plain.push(' ');
            }
            '-' if chars.peek() == Some(&'-') => {
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        break;
                    }
                }
                plain.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut star = false;
                for inner in chars.by_ref() {
                    if star && inner == '/' {
                        break;
                    }
                    star = inner == '*';
                }
                plain.push(' ');
            }
            other => plain.push(other),
        }
    }
    plain
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_uppercase)
        .collect()
}

pub(super) fn unrestated(definition: Option<&str>, column: &str) -> Option<String> {
    let definition = definition?;
    let quoted = format!("`{}`", column.replace('`', "``"));
    let line = definition.lines().map(str::trim).find(|line| line.starts_with(&quoted))?;
    let upper = line[quoted.len()..].to_ascii_uppercase();
    NOT_RESTATED.iter().find(|attribute| upper.contains(**attribute)).map(|attribute| (*attribute).to_owned())
}

fn literal_default(default: &str) -> bool {
    let text = default.trim();
    let upper = text.to_ascii_uppercase();
    text.starts_with('(')
        || (text.starts_with('\'') && text.ends_with('\''))
        || text.parse::<f64>().is_ok()
        || matches!(upper.as_str(), "NULL" | "TRUE" | "FALSE" | "CURRENT_TIME" | "CURRENT_DATE" | "CURRENT_TIMESTAMP")
}

impl Writer {
    fn body(&self, object: &Object) -> Result<Vec<String>, Refusal> {
        let key = object.constraints.iter().find(|constraint| constraint.kind == ConstraintKind::PrimaryKey);
        let key_columns: Vec<String> = key.map(|key| key.columns.clone()).unwrap_or_default();
        let inline = key_columns.len() == 1
            && object.columns.iter().any(|column| column.name == key_columns[0] && column.identity.is_some());
        let mut lines = Vec::new();
        for column in &object.columns {
            let mut line = format!("{} {}", self.q(&column.name), column.type_name.trim());
            if inline && column.name == key_columns[0] {
                line.push_str(" PRIMARY KEY AUTOINCREMENT");
            } else if !column.nullable && !key_columns.contains(&column.name) {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = column.default.as_deref().filter(|default| !default.trim().is_empty()) {
                if literal_default(default) {
                    line.push_str(&format!(" DEFAULT {}", default.trim()));
                } else {
                    line.push_str(&format!(" DEFAULT ({})", default.trim()));
                }
            }
            lines.push(line);
        }
        for constraint in &object.constraints {
            match constraint.kind {
                ConstraintKind::PrimaryKey if !inline => {
                    lines.push(format!("PRIMARY KEY ({})", self.columns(&constraint.columns)?));
                }
                ConstraintKind::Unique => lines.push(format!("UNIQUE ({})", self.columns(&constraint.columns)?)),
                ConstraintKind::Check => lines.push(constraint.definition.clone()),
                ConstraintKind::ForeignKey => {
                    let Some(reference) = &constraint.references else { continue };
                    let mut line = format!(
                        "FOREIGN KEY ({}) REFERENCES {}",
                        self.columns(&constraint.columns)?,
                        self.q(&reference.table)
                    );
                    if !reference.columns.is_empty() {
                        line.push_str(&format!(" ({})", self.columns(&reference.columns)?));
                    }
                    if let Some(action) =
                        self.action(Some(&reference.on_delete))?.filter(|action| *action != "NO ACTION")
                    {
                        line.push_str(&format!(" ON DELETE {action}"));
                    }
                    if let Some(action) =
                        self.action(Some(&reference.on_update))?.filter(|action| *action != "NO ACTION")
                    {
                        line.push_str(&format!(" ON UPDATE {action}"));
                    }
                    lines.push(line);
                }
                _ => {}
            }
        }
        Ok(lines)
    }

    pub(super) fn rebuild(
        &mut self,
        schema: &str,
        table: &str,
        edit: impl FnOnce(&mut Object) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        let ours = self.rebuilt.iter().any(|(known, name)| known == schema && *name == object.name);
        if !ours {
            let Some(definition) = object.definition.as_deref() else {
                return Err(self.refuse(Reason::UnknownObject, object.name.clone()));
            };
            let found = words(definition);
            if let Some(lost) = LOST.iter().find(|lost| found.iter().any(|word| word == *lost)) {
                let subject = match *lost {
                    "WITHOUT" => "WITHOUT ROWID",
                    "AS" => "GENERATED",
                    other => other,
                };
                return Err(self.refuse(Reason::RebuildLoses, subject));
            }
            if object.columns.iter().any(|column| column.generated.is_some()) {
                return Err(self.refuse(Reason::RebuildLoses, "GENERATED"));
            }
        }
        let stale = self.renamed.iter().any(|(known, name)| known == schema && *name == object.name);
        if stale && !object.triggers.is_empty() {
            return Err(self.refuse(Reason::RunFirst, object.name.clone()));
        }
        if schema != "main" && !object.triggers.is_empty() {
            return Err(self.refuse(Reason::RebuildLoses, "TRIGGER"));
        }
        let regenerate = schema != "main" || stale;
        if regenerate
            && let Some(index) = object.indexes.iter().find(|index| {
                !index.definition.trim().is_empty()
                    && (words(&index.definition).iter().any(|word| word == "WHERE")
                        || index.columns.iter().any(|column| column.starts_with('<')))
            })
        {
            let reason = if stale { Reason::RunFirst } else { Reason::RebuildLoses };
            return Err(self.refuse(reason, index.name.clone()));
        }
        let before: Vec<String> = object.columns.iter().map(|column| column.name.clone()).collect();
        edit(&mut object)?;
        let name = object.name.clone();
        let temporary = format!("{TEMPORARY}{name}");
        if self.exists(schema, &temporary) {
            return Err(self.refuse(Reason::Exists, temporary));
        }
        let body = self.body(&object)?.join(",\n    ");
        let copied: Vec<String> =
            object.columns.iter().map(|column| column.name.clone()).filter(|column| before.contains(column)).collect();
        if !self.script.before.iter().any(|statement| statement.starts_with("PRAGMA foreign_keys")) {
            self.script.before.push("PRAGMA foreign_keys = OFF".to_owned());
        }
        self.push("PRAGMA legacy_alter_table = ON".to_owned());
        self.push(format!("CREATE TABLE {} (\n    {body}\n)", self.table(schema, &temporary)));
        if !copied.is_empty() {
            let list = self.columns(&copied)?;
            self.push(format!(
                "INSERT INTO {} ({list})\nSELECT {list} FROM {}",
                self.table(schema, &temporary),
                self.table(schema, &name)
            ));
        }
        self.push(format!("DROP TABLE {}", self.table(schema, &name)));
        self.push(format!("ALTER TABLE {} RENAME TO {}", self.table(schema, &temporary), self.q(&name)));
        for index in object.indexes.iter().filter(|index| !index.definition.trim().is_empty()) {
            let statement = if !regenerate {
                index.definition.trim().trim_end_matches(';').to_owned()
            } else {
                let unique = if index.unique { "UNIQUE " } else { "" };
                format!(
                    "CREATE {unique}INDEX {} ON {} ({})",
                    self.table(schema, &index.name),
                    self.q(&name),
                    self.columns(&index.columns)?
                )
            };
            self.push(statement);
        }
        for trigger in &object.triggers {
            self.push(trigger.definition.trim().trim_end_matches(';').to_owned());
        }
        self.push("PRAGMA legacy_alter_table = OFF".to_owned());
        self.script.notes.push(Note::Rebuild { table: name.clone() });
        object.definition = Some(format!("CREATE TABLE {} (\n    {body}\n)", self.q(&name)));
        self.rebuilt.push((schema.to_owned(), name));
        self.store(schema, object);
        Ok(())
    }
}
