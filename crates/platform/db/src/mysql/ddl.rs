use super::quote_ident;
use crate::driver::{DbError, ErrorKind};
use crate::schema::{ConstraintKind, DdlOptions, Object, ObjectKind, Snapshot, find_object};

fn delimited(statement: &str) -> String {
    let statement = statement.trim().trim_end_matches(';').trim_end();
    if statement.contains(';') { format!("DELIMITER $$\n{statement}$$\nDELIMITER ;") } else { format!("{statement};") }
}

fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''"))
}

fn generated_table(object: &Object) -> String {
    let mut lines: Vec<String> = object
        .columns
        .iter()
        .map(|column| {
            let mut line = format!("  {} {}", quote_ident(&column.name), column.type_name);
            if let Some(expression) = &column.generated {
                line.push_str(&format!(" GENERATED ALWAYS AS {expression}"));
            }
            if let Some(collation) = &column.collation {
                line.push_str(&format!(" COLLATE {collation}"));
            }
            if !column.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = &column.default {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            if column.identity.is_some() {
                line.push_str(" AUTO_INCREMENT");
            }
            if let Some(comment) = &column.comment {
                line.push_str(&format!(" COMMENT {}", literal(comment)));
            }
            line
        })
        .collect();
    for index in &object.indexes {
        let columns = index_columns(index);
        lines.push(if index.primary {
            format!("  PRIMARY KEY ({columns})")
        } else {
            let kind = match index.method.as_str() {
                "fulltext" => "FULLTEXT KEY",
                "spatial" => "SPATIAL KEY",
                _ if index.unique => "UNIQUE KEY",
                _ => "KEY",
            };
            format!("  {kind} {} ({columns})", quote_ident(&index.name))
        });
    }
    for constraint in &object.constraints {
        if matches!(constraint.kind, ConstraintKind::ForeignKey | ConstraintKind::Check) {
            lines.push(format!("  CONSTRAINT {} {}", quote_ident(&constraint.name), constraint.definition));
        }
    }
    let mut statement = format!("CREATE TABLE {} (\n{}\n)", quote_ident(&object.name), lines.join(",\n"));
    if let Some(comment) = &object.comment {
        statement.push_str(&format!(" COMMENT={}", literal(comment)));
    }
    statement
}

fn index_columns(index: &crate::schema::Index) -> String {
    index
        .definition
        .rsplit_once(" (")
        .map(|(_, columns)| columns.trim_end_matches(')').to_owned())
        .filter(|columns| !columns.is_empty())
        .unwrap_or_else(|| index.columns.iter().map(|column| quote_ident(column)).collect::<Vec<_>>().join(", "))
}

pub(super) fn generate(snapshot: &Snapshot, path: &[String], options: &DdlOptions) -> Result<String, DbError> {
    let found = find_object(snapshot, path).ok_or_else(|| DbError::new(ErrorKind::Invalid, path.join(".")))?;
    let object = found.object;
    if !object.detailed {
        return Err(DbError::new(ErrorKind::Unsupported, format!("{}: details not read yet", object.name)));
    }
    let table = format!("{}.{}", quote_ident(&found.schema.name), quote_ident(&object.name));
    let parts = match (found.sub, found.item) {
        (Some("indexes"), Some(item)) => {
            let index = object.indexes.iter().find(|index| index.name == item);
            vec![format!("{};", index.ok_or_else(|| DbError::new(ErrorKind::Invalid, item))?.definition)]
        }
        (Some("constraints" | "foreignKeys"), Some(item)) => {
            let constraint = object
                .constraints
                .iter()
                .find(|constraint| constraint.name == item)
                .ok_or_else(|| DbError::new(ErrorKind::Invalid, item))?;
            vec![match constraint.kind {
                ConstraintKind::PrimaryKey => format!("ALTER TABLE {table} ADD {};", constraint.definition),
                _ => format!(
                    "ALTER TABLE {table} ADD CONSTRAINT {} {};",
                    quote_ident(&constraint.name),
                    constraint.definition
                ),
            }]
        }
        (Some("triggers"), Some(item)) => {
            let trigger = object.triggers.iter().find(|trigger| trigger.name == item);
            vec![delimited(&trigger.ok_or_else(|| DbError::new(ErrorKind::Invalid, item))?.definition)]
        }
        (Some(_), Some(item)) => return Err(DbError::new(ErrorKind::Unsupported, item)),
        _ => {
            let own = match object.kind {
                ObjectKind::Function | ObjectKind::Procedure => {
                    object.routine.as_ref().and_then(|routine| routine.definition.clone())
                }
                ObjectKind::Table => Some(object.definition.clone().unwrap_or_else(|| generated_table(object))),
                _ => object.definition.clone(),
            };
            let own =
                own.ok_or_else(|| DbError::new(ErrorKind::Unsupported, format!("{}: no definition", object.name)))?;
            let mut parts = vec![delimited(&own)];
            if options.nested {
                parts.extend(object.triggers.iter().map(|trigger| delimited(&trigger.definition)));
            }
            parts
        }
    };
    let mut text = parts.join("\n\n");
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Catalog, Column, Constraint, Index, Level, Reference, Routine, Schema, Trigger};

    fn snapshot() -> Snapshot {
        let mut orders = Object::new(ObjectKind::Table, "orders");
        orders.detailed = true;
        orders.comment = Some("Customer's orders".into());
        orders.columns = vec![
            Column {
                name: "id".into(),
                position: 1,
                type_name: "int".into(),
                identity: Some("byDefault".into()),
                ..Column::default()
            },
            Column {
                name: "customer".into(),
                position: 2,
                type_name: "int".into(),
                nullable: true,
                comment: Some("who".into()),
                ..Column::default()
            },
        ];
        orders.indexes = vec![
            Index {
                name: "PRIMARY".into(),
                columns: vec!["id".into()],
                unique: true,
                primary: true,
                method: "btree".into(),
                definition: "ALTER TABLE `shop`.`orders` ADD PRIMARY KEY (`id`)".into(),
                ..Index::default()
            },
            Index {
                name: "by_customer".into(),
                columns: vec!["customer".into()],
                method: "btree".into(),
                definition: "CREATE INDEX `by_customer` ON `shop`.`orders` (`customer`)".into(),
                ..Index::default()
            },
        ];
        orders.constraints = vec![Constraint {
            name: "orders_customer".into(),
            kind: ConstraintKind::ForeignKey,
            columns: vec!["customer".into()],
            definition:
                "FOREIGN KEY (`customer`) REFERENCES `shop`.`customers` (`id`) ON DELETE CASCADE ON UPDATE NO ACTION"
                    .into(),
            references: Some(Reference { schema: "shop".into(), table: "customers".into(), ..Reference::default() }),
            index: None,
            comment: None,
        }];
        orders.triggers = vec![Trigger {
            name: "stamp".into(),
            definition: "CREATE TRIGGER `stamp` BEFORE INSERT ON `orders` FOR EACH ROW BEGIN SET NEW.customer = 1; END"
                .into(),
            enabled: true,
            function: "BEFORE INSERT".into(),
        }];
        let mut total = Object::new(ObjectKind::Function, "total");
        total.detailed = true;
        total.routine = Some(Routine {
            arguments: "a int".into(),
            result: Some("int".into()),
            language: "SQL".into(),
            kind: "function".into(),
            definition: Some("CREATE FUNCTION `total`(a int) RETURNS int DETERMINISTIC RETURN a + 1".into()),
        });
        Snapshot {
            catalogs: vec![Catalog {
                name: "def".into(),
                current: true,
                schemas_read: true,
                schemas: vec![Schema {
                    name: "shop".into(),
                    level: Level::Details,
                    objects: vec![orders, total],
                    ..Schema::default()
                }],
                ..Catalog::default()
            }],
            ..Snapshot::new("my", "mysql")
        }
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn tables_are_generated_when_show_create_gave_nothing() {
        let ddl = generate(&snapshot(), &path(&["def", "shop", "tables", "orders"]), &DdlOptions::default()).unwrap();
        assert_eq!(
            ddl,
            "CREATE TABLE `orders` (\n  `id` int NOT NULL AUTO_INCREMENT,\n  `customer` int COMMENT 'who',\n  PRIMARY KEY (`id`),\n  \
             KEY `by_customer` (`customer`),\n  CONSTRAINT `orders_customer` FOREIGN KEY (`customer`) REFERENCES `shop`.`customers` (`id`) \
             ON DELETE CASCADE ON UPDATE NO ACTION\n) COMMENT='Customer''s orders';\n"
        );
        let nested = DdlOptions { nested: true, ..DdlOptions::default() };
        let ddl = generate(&snapshot(), &path(&["def", "shop", "tables", "orders"]), &nested).unwrap();
        assert!(ddl.ends_with("DELIMITER $$\nCREATE TRIGGER `stamp` BEFORE INSERT ON `orders` FOR EACH ROW BEGIN SET NEW.customer = 1; END$$\nDELIMITER ;\n"));
    }

    #[test]
    fn routines_and_parts_of_tables_have_their_own_statements() {
        let snapshot = snapshot();
        let routine = generate(&snapshot, &path(&["def", "shop", "functions", "total(a int)"]), &DdlOptions::default());
        assert_eq!(routine.unwrap(), "CREATE FUNCTION `total`(a int) RETURNS int DETERMINISTIC RETURN a + 1;\n");
        let key = generate(
            &snapshot,
            &path(&["def", "shop", "tables", "orders", "foreignKeys", "orders_customer"]),
            &DdlOptions::default(),
        );
        assert!(key.unwrap().starts_with("ALTER TABLE `shop`.`orders` ADD CONSTRAINT `orders_customer` FOREIGN KEY"));
        let index = generate(
            &snapshot,
            &path(&["def", "shop", "tables", "orders", "indexes", "PRIMARY"]),
            &DdlOptions::default(),
        );
        assert_eq!(index.unwrap(), "ALTER TABLE `shop`.`orders` ADD PRIMARY KEY (`id`);\n");
    }
}
