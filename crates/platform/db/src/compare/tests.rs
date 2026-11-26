use super::data::{DataScript, RowDiff, data_script, differing, key_text, lookup_query, page_query, same_value};
use super::*;
use crate::dialect::SqlDialect;
use crate::schema::{Column, Constraint, ConstraintKind, Index, Object, ObjectKind, Reference, Schema, Trigger};
use crate::value::Value;

fn column(name: &str, type_name: &str, nullable: bool) -> Column {
    Column { name: name.to_owned(), type_name: type_name.to_owned(), nullable, ..Column::default() }
}

fn table(name: &str, columns: Vec<Column>) -> Object {
    let mut object = Object::new(ObjectKind::Table, name);
    object.detailed = true;
    object.columns = columns;
    object
}

fn key(name: &str, columns: &[&str]) -> Constraint {
    Constraint {
        name: name.to_owned(),
        kind: ConstraintKind::PrimaryKey,
        columns: columns.iter().map(|column| (*column).to_owned()).collect(),
        definition: format!("PRIMARY KEY ({})", columns.join(", ")),
        references: None,
        index: Some(name.to_owned()),
        comment: None,
    }
}

fn schema(name: &str, objects: Vec<Object>) -> Schema {
    Schema { name: name.to_owned(), objects, ..Schema::default() }
}

fn shop(name: &str) -> (Schema, Schema) {
    let mut orders = table(
        "orders",
        vec![
            column("id", "integer", false),
            column("note", "character varying(40)", true),
            column("old", "text", true),
        ],
    );
    orders.constraints.push(key("orders_pkey", &["id"]));
    let mut orders_new = table(
        "orders",
        vec![
            column("id", "integer", false),
            column("note", "character varying(80)", false),
            Column { default: Some("now()".to_owned()), ..column("created", "timestamp with time zone", true) },
        ],
    );
    orders_new.constraints.push(key("pk_orders", &["id"]));
    orders_new.constraints.push(Constraint {
        name: "orders_customer_fkey".to_owned(),
        kind: ConstraintKind::ForeignKey,
        columns: vec!["customer".to_owned()],
        definition: format!("FOREIGN KEY (customer) REFERENCES {name}.customers(id)"),
        references: Some(Reference {
            schema: name.to_owned(),
            table: "customers".to_owned(),
            columns: vec!["id".to_owned()],
            on_update: "no action".to_owned(),
            on_delete: "no action".to_owned(),
        }),
        index: None,
        comment: None,
    });
    orders_new.indexes.push(Index {
        name: "orders_note".to_owned(),
        columns: vec!["note".to_owned()],
        method: "btree".to_owned(),
        definition: format!("CREATE INDEX orders_note ON {name}.orders USING btree (note)"),
        ..Index::default()
    });
    orders_new.indexes.push(Index {
        name: "pk_orders".to_owned(),
        columns: vec!["id".to_owned()],
        unique: true,
        primary: true,
        method: "btree".to_owned(),
        definition: format!("CREATE UNIQUE INDEX pk_orders ON {name}.orders USING btree (id)"),
        constraint: Some("pk_orders".to_owned()),
        ..Index::default()
    });
    let customers = table("customers", vec![column("id", "integer", false)]);
    let legacy = table("legacy", vec![column("id", "integer", false)]);
    let mut view = Object::new(ObjectKind::View, "recent");
    view.detailed = true;
    view.definition = Some(" SELECT id\n   FROM public.orders;".to_owned());
    let mut view_new = view.clone();
    view_new.definition = Some(format!("SELECT id FROM {name}.orders WHERE id > 10"));
    (schema("public", vec![orders, legacy, view]), schema(name, vec![orders_new, customers, view_new]))
}

fn words(note: &Note) -> String {
    format!("{:?} {} {}", note.kind, note.object, note.part)
}

#[test]
fn schemas_compare_by_object_and_part_and_keys_match_by_what_they_cover_not_their_names() {
    let (target, source) = shop("app");
    let diff = compare(&source, &target, &CompareOptions::default());
    let summary: Vec<(ObjectKind, &str, Change)> =
        diff.objects.iter().map(|object| (object.kind, object.name.as_str(), object.change)).collect();
    assert_eq!(
        summary,
        vec![
            (ObjectKind::Table, "customers", Change::Added),
            (ObjectKind::Table, "legacy", Change::Removed),
            (ObjectKind::Table, "orders", Change::Changed),
            (ObjectKind::View, "recent", Change::Changed),
        ]
    );
    let orders = &diff.objects[2];
    let parts: Vec<(Part, &str, Change, Vec<String>)> =
        orders.items.iter().map(|item| (item.part, item.name.as_str(), item.change, item.fields.clone())).collect();
    assert_eq!(
        parts,
        vec![
            (Part::Column, "note", Change::Changed, vec!["type".to_owned(), "nullable".to_owned()]),
            (Part::Column, "created", Change::Added, vec![]),
            (Part::Column, "old", Change::Removed, vec![]),
            (Part::ForeignKey, "orders_customer_fkey", Change::Added, vec![]),
            (Part::Index, "orders_note", Change::Added, vec![]),
        ],
        "the primary keys pair although their names differ, and the key's own index is not an index difference"
    );
    assert_eq!(diff.same, 0);
    assert_eq!(diff.counts(), (1, 1, 2));
    let unchanged = compare(&source, &source, &CompareOptions::default());
    assert!(unchanged.objects.is_empty());
    assert_eq!(unchanged.same, 3);
}

#[test]
fn a_view_query_differing_only_in_whitespace_and_its_schema_qualifier_is_the_same() {
    let mut source = Object::new(ObjectKind::View, "v");
    source.detailed = true;
    source.definition = Some("SELECT id\n  FROM app.orders;".to_owned());
    let mut target = source.clone();
    target.definition = Some(" SELECT id FROM \"public\".orders".to_owned());
    let diff = compare(&schema("app", vec![source]), &schema("public", vec![target]), &CompareOptions::default());
    assert!(diff.objects.is_empty(), "{diff:?}");
}

#[test]
fn comments_kinds_and_objects_without_details_follow_the_options() {
    let mut source = table("t", vec![Column { comment: Some("new".to_owned()), ..column("a", "int", true) }]);
    source.comment = Some("table".to_owned());
    let target = table("t", vec![column("a", "int", true)]);
    let mut seq = Object::new(ObjectKind::Sequence, "s");
    seq.detailed = false;
    let (source, target) = (schema("s", vec![source, seq.clone()]), schema("s", vec![target, seq]));
    let diff = compare(&source, &target, &CompareOptions::default());
    assert_eq!(diff.objects.len(), 1);
    assert_eq!(diff.objects[0].items.len(), 2, "a column's comment and the table's");
    assert_eq!(diff.partial, 1, "the sequence was compared by name only");
    let quiet = compare(&source, &target, &CompareOptions { ignore_comments: true, ..CompareOptions::default() });
    assert!(quiet.objects.is_empty());
    let only =
        compare(&source, &target, &CompareOptions { kinds: vec![ObjectKind::Sequence], ..CompareOptions::default() });
    assert_eq!((only.objects.len(), only.same), (0, 1));
}

#[test]
fn requalify_changes_only_qualifications_by_that_schema() {
    let text = "CREATE INDEX i ON app.orders (app_id); SELECT \"app\".x, `app`.y, myapp.z";
    assert_eq!(
        requalify(text, "app", Some("public")),
        "CREATE INDEX i ON public.orders (app_id); SELECT \"public\".x, `public`.y, myapp.z"
    );
    assert_eq!(requalify(text, "app", None), "CREATE INDEX i ON orders (app_id); SELECT x, y, myapp.z");
}

fn create(schema: &Schema, object: &Object) -> Option<String> {
    generic_create(SqlDialect::Postgres, &schema.name, object)
}

#[test]
fn the_postgres_sync_script_creates_alters_and_leaves_drops_as_notes_unless_asked() {
    let (target, source) = shop("app");
    let diff = compare(&source, &target, &CompareOptions::default());
    let script =
        sync_script(&diff, (&source, &target), SqlDialect::Postgres, &create, &words, &ScriptOptions::default());
    let expected = "\
-- NotDropped \"public\".\"legacy\" \n\
CREATE TABLE \"public\".\"customers\" (\n    \"id\" integer NOT NULL\n);\n\
ALTER TABLE \"public\".\"orders\" ADD COLUMN \"created\" timestamp with time zone DEFAULT now();\n\
ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"note\" TYPE character varying(80);\n\
ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"note\" SET NOT NULL;\n\
-- NotDropped \"public\".\"orders\" old\n\
CREATE VIEW \"public\".\"recent\" AS\nSELECT id FROM public.orders WHERE id > 10;\n\
CREATE INDEX orders_note ON public.orders USING btree (note);\n\
ALTER TABLE \"public\".\"orders\" ADD CONSTRAINT \"orders_customer_fkey\" FOREIGN KEY (customer) REFERENCES public.customers(id);\n";
    assert_eq!(script.text, expected);
    assert_eq!(script.statements, 7);
    assert_eq!(script.notes.len(), 2);
    let dropping =
        sync_script(&diff, (&source, &target), SqlDialect::Postgres, &create, &words, &ScriptOptions { drop: true });
    assert!(dropping.text.starts_with("DROP TABLE \"public\".\"legacy\";\n"), "{}", dropping.text);
    assert!(dropping.text.contains("ALTER TABLE \"public\".\"orders\" DROP COLUMN \"old\";\n"), "{}", dropping.text);
    assert!(dropping.notes.is_empty());
}

#[test]
fn mysql_modifies_whole_columns_and_sqlite_leaves_what_it_cannot_alter_as_notes() {
    let source = schema("shop", vec![table("t", vec![column("a", "varchar(80)", false)])]);
    let target = schema("shop", vec![table("t", vec![column("a", "varchar(40)", true)])]);
    let diff = compare(&source, &target, &CompareOptions::default());
    let none = |_: &Schema, _: &Object| None;
    let mysql = sync_script(&diff, (&source, &target), SqlDialect::MySql, &none, &words, &ScriptOptions::default());
    assert_eq!(mysql.text, "USE `shop`;\nALTER TABLE `shop`.`t` MODIFY COLUMN `a` varchar(80) NOT NULL;\n");
    let sqlite = sync_script(&diff, (&source, &target), SqlDialect::Sqlite, &none, &words, &ScriptOptions::default());
    assert_eq!(sqlite.text, "-- CannotAlter \"shop\".\"t\" a\n");
    assert_eq!(sqlite.statements, 0);
}

#[test]
fn triggers_and_changed_keys_are_dropped_and_written_again() {
    let mut source = table("t", vec![column("a", "int", false)]);
    source.constraints.push(Constraint { definition: "UNIQUE (a, b)".to_owned(), ..key("t_a", &["a"]) });
    source.constraints[0].kind = ConstraintKind::Unique;
    source.triggers.push(Trigger {
        name: "audit".to_owned(),
        definition: "CREATE TRIGGER audit AFTER INSERT ON s.t FOR EACH ROW EXECUTE FUNCTION f()".to_owned(),
        enabled: true,
        function: "f".to_owned(),
    });
    let mut target = table("t", vec![column("a", "int", false)]);
    target.constraints.push(Constraint { definition: "UNIQUE (a)".to_owned(), ..key("t_a", &["a"]) });
    target.constraints[0].kind = ConstraintKind::Unique;
    let (source, target) = (schema("s", vec![source]), schema("s", vec![target]));
    let diff = compare(&source, &target, &CompareOptions::default());
    let script =
        sync_script(&diff, (&source, &target), SqlDialect::Postgres, &create, &words, &ScriptOptions::default());
    assert_eq!(
        script.text,
        "ALTER TABLE \"s\".\"t\" DROP CONSTRAINT \"t_a\";\n\
ALTER TABLE \"s\".\"t\" ADD CONSTRAINT \"t_a\" UNIQUE (a, b);\n\
CREATE TRIGGER audit AFTER INSERT ON s.t FOR EACH ROW EXECUTE FUNCTION f();\n"
    );
}

#[test]
fn data_pages_read_past_the_last_key_and_lookups_name_each_key() {
    let key = ["id".to_owned()];
    let columns = ["id".to_owned(), "name".to_owned()];
    assert_eq!(
        page_query(SqlDialect::Postgres, "public.t", (&key, &columns), None, 500),
        "SELECT \"id\", \"name\" FROM public.t ORDER BY \"id\" LIMIT 500"
    );
    assert_eq!(
        page_query(SqlDialect::MySql, "`s`.`t`", (&key, &columns), Some(&[Value::Integer(7)]), 500),
        "SELECT `id`, `name` FROM `s`.`t` WHERE `id` > 7 ORDER BY `id` LIMIT 500"
    );
    let pair = ["a".to_owned(), "b".to_owned()];
    assert_eq!(
        page_query(
            SqlDialect::Sqlite,
            "t",
            (&pair, &pair),
            Some(&[Value::Integer(1), Value::Text("x'y".to_owned())]),
            10
        ),
        "SELECT \"a\", \"b\" FROM t WHERE (\"a\", \"b\") > (1, 'x''y') ORDER BY \"a\", \"b\" LIMIT 10"
    );
    assert_eq!(
        lookup_query(SqlDialect::Postgres, "t", (&key, &columns), &[vec![Value::Integer(1)], vec![Value::Integer(2)]]),
        "SELECT \"id\", \"name\" FROM t WHERE \"id\" IN (1, 2)"
    );
    assert_eq!(
        lookup_query(
            SqlDialect::Postgres,
            "t",
            (&pair, &pair),
            &[vec![Value::Integer(1), Value::Text("x".to_owned())]]
        ),
        "SELECT \"a\", \"b\" FROM t WHERE (\"a\" = 1 AND \"b\" = 'x')"
    );
}

#[test]
fn values_from_two_servers_compare_by_meaning() {
    assert!(same_value(&Value::Integer(5), &Value::Decimal("5".to_owned())));
    assert!(same_value(&Value::Decimal("5.50".to_owned()), &Value::Float(5.5)));
    assert!(same_value(&Value::Bool(true), &Value::Integer(1)));
    assert!(!same_value(&Value::Null, &Value::Text(String::new())));
    assert!(same_value(&Value::Null, &Value::Null));
    assert_eq!(key_text(&[Value::Integer(3)]), key_text(&[Value::Decimal("3".to_owned())]));
    let columns = ["id".to_owned(), "name".to_owned()];
    assert_eq!(
        differing(
            &columns,
            &[Value::Integer(1), Value::Text("a".to_owned())],
            &[Value::Integer(1), Value::Text("b".to_owned())]
        ),
        vec!["name".to_owned()]
    );
}

#[test]
fn the_data_script_inserts_updates_and_deletes_only_when_asked() {
    let key = ["id".to_owned()];
    let columns = ["id".to_owned(), "name".to_owned()];
    let diffs = vec![
        RowDiff {
            change: Change::Added,
            key: vec![Value::Integer(1)],
            columns: Vec::new(),
            source: Some(vec![Value::Integer(1), Value::Text("O'Neil".to_owned())]),
            target: None,
        },
        RowDiff {
            change: Change::Changed,
            key: vec![Value::Integer(2)],
            columns: vec!["name".to_owned()],
            source: Some(vec![Value::Integer(2), Value::Null]),
            target: Some(vec![Value::Integer(2), Value::Text("b".to_owned())]),
        },
        RowDiff {
            change: Change::Removed,
            key: vec![Value::Integer(3)],
            columns: Vec::new(),
            source: None,
            target: Some(vec![Value::Integer(3), Value::Text("c".to_owned())]),
        },
    ];
    let kept = data_script(SqlDialect::Postgres, "public.t", (&key, &columns), &diffs, false);
    assert_eq!(
        kept,
        DataScript {
            text: "INSERT INTO public.t (\"id\", \"name\") VALUES (1, 'O''Neil');\nUPDATE public.t SET \"name\" = NULL WHERE \"id\" = 2;\n"
                .to_owned(),
            statements: 2,
            not_deleted: 1,
        }
    );
    let all = data_script(SqlDialect::MySql, "t", (&key, &columns), &diffs, true);
    assert!(all.text.ends_with("DELETE FROM t WHERE `id` = 3;\n"), "{}", all.text);
    assert_eq!(all.statements, 3);
}
