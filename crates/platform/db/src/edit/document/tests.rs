use super::*;
use crate::edit::{Deleted, Inserted, Updated};

fn columns() -> Vec<Column> {
    vec![
        Column::new("_id", "objectId"),
        Column::new("name", "string"),
        Column::new("age", "int"),
        Column::new("views", "long"),
        Column::new("tags", "array"),
    ]
}

fn users() -> Target {
    Target { database: None, collection: "users".into() }
}

fn id() -> Value {
    Value::Other(Typed { text: "ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\")".into(), type_name: "objectId".into() })
}

fn row() -> Vec<Value> {
    vec![id(), Value::Text("Ada".into()), Value::Integer(36), Value::Integer(7), Value::Json("[\"a\"]".into())]
}

#[test]
fn the_collection_of_a_find_is_known_and_other_statements_have_none() {
    let named = |statement: &str| collection_of(statement).map(|target| target.name());
    assert_eq!(named("db.users.find({ age: { $gt: 30 } }).sort({ name: 1 }).limit(10);").as_deref(), Some("users"));
    assert_eq!(named("db.getCollection('my orders').find()").as_deref(), Some("my orders"));
    assert_eq!(named("db.users.findOne({ name: \"a)b\" })").as_deref(), Some("users"));
    let opened = collection_of(&find_text("shop", "system.views")).unwrap();
    assert_eq!((opened.database.as_deref(), opened.collection.as_str()), (Some("shop"), "system.views"));
    assert_eq!(opened.expression(), "db.getSiblingDB(\"shop\").getCollection(\"system.views\")");
    assert_eq!(collection_of("db.users.find().count()"), None);
    assert_eq!(collection_of("db.users.aggregate([])"), None);
    assert_eq!(collection_of("db.users.find() extra"), None);
    assert_eq!(collection_of("show dbs"), None);
}

#[test]
fn documents_are_identified_by_id_which_is_not_editable() {
    let editing = identify(&columns(), &users());
    assert!(editing.editable && editing.documents);
    assert_eq!(editing.key.as_ref().map(|key| key.columns.clone()), Some(vec![0]));
    assert!(!editing.columns[0].editable && editing.columns[1].editable);
    let without = identify(&columns()[1..], &users());
    assert_eq!((without.editable, without.reason), (false, Some(ReadOnly::NoKey)));
}

#[test]
fn typed_input_follows_the_bson_type_of_the_field() {
    assert_eq!(parse_input("42", "int"), Ok(Value::Integer(42)));
    assert_eq!(parse_input("x", "int"), Err(InputError::Integer));
    assert_eq!(parse_input(" spaced ", "string"), Ok(Value::Text(" spaced ".into())));
    assert_eq!(parse_input("{\"a\": 1}", "object"), Ok(Value::Json("{\"a\": 1}".into())));
    assert_eq!(parse_input("[1]", "object"), Err(InputError::Json));
    assert_eq!(parse_input("6AE0B2AD2EE72A7B6191F5A9", "objectId"), Ok(id()));
    assert_eq!(parse_input("ObjectId('6ae0b2ad2ee72a7b6191f5a9')", "objectId"), Ok(id()));
    assert_eq!(parse_input("123", "objectId"), Err(InputError::ObjectId));
    assert_eq!(parse_input("2.5", "mixed"), Ok(Value::Float(2.5)));
    assert_eq!(parse_input("true", "null"), Ok(Value::Bool(true)));
    assert_eq!(parse_input("hello", "mixed"), Ok(Value::Text("hello".into())));
}

#[test]
fn values_are_written_as_the_shell_reads_them() {
    assert_eq!(literal(&Value::Integer(5), "long"), "NumberLong(\"5\")");
    assert_eq!(literal(&Value::Integer(5), "double"), "5.0");
    assert_eq!(literal(&Value::Text("a\"b".into()), "string"), "\"a\\\"b\"");
    assert_eq!(literal(&Value::Bytes(vec![1, 2, 3, 4]), "binData"), "BinData(0, \"AQIDBA==\")");
    assert_eq!(literal(&Value::Float(f64::NAN), "double"), "{\"$numberDouble\": \"NaN\"}");
    let date = Value::Temporal(Typed { text: "2026-10-28T00:00:00Z".into(), type_name: "date".into() });
    assert_eq!(literal(&date, "date"), "ISODate(\"2026-10-28T00:00:00Z\")");
}

#[test]
fn new_object_ids_are_unique_hexadecimal() {
    let (first, second) = (new_object_id(), new_object_id());
    assert!(is_object_id(&first) && is_object_id(&second));
    assert_ne!(first, second);
}

#[test]
fn changes_are_saved_with_delete_insert_and_update_by_id() {
    let editing = identify(&columns(), &users());
    let changes = Changes {
        deleted: vec![Deleted { row: 1, original: row() }],
        inserted: vec![Inserted {
            row: 2,
            cells: vec![(1, Staged::Value(Value::Text("Bob".into()))), (3, Staged::Default)],
        }],
        updated: vec![Updated {
            row: 0,
            original: row(),
            cells: vec![(2, Staged::Value(Value::Integer(37))), (4, Staged::Default)],
        }],
    };
    let planned = plan(&editing, &changes);
    assert_eq!(planned.len(), 3);
    assert_eq!(
        planned[0].sql,
        "db.getCollection(\"users\").deleteOne({ \"_id\": ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\") })"
    );
    assert!(planned[1].sql.starts_with("db.getCollection(\"users\").insertOne({ \"_id\": ObjectId(\""));
    assert!(planned[1].sql.ends_with("\"name\": \"Bob\" })"));
    assert_eq!(planned[1].returning, vec![0]);
    let fetch = planned[1].fetch.as_ref().unwrap();
    assert!(fetch.sql.contains(".find({ \"_id\": ObjectId(") && fetch.sql.ends_with("{ \"_id\": 1 })"));
    assert_eq!(
        planned[2].sql,
        "db.getCollection(\"users\").updateOne({ \"_id\": ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\"), \"age\": 36 }, \
         { \"$set\": { \"age\": 37 }, \"$unset\": { \"tags\": \"\" } })"
    );
    assert!(planned.iter().all(|statement| statement.one_row && statement.params.is_empty()));
}

#[test]
fn generated_statements_find_and_change_rows_by_id() {
    let editing = identify(&columns(), &users());
    let text = generate(&editing, Generate::Update, &[row()]).unwrap();
    assert_eq!(
        text,
        "db.getCollection(\"users\").updateOne({ \"_id\": ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\") }, { \"$set\": \
         { \"name\": \"Ada\", \"age\": 36, \"views\": NumberLong(\"7\"), \"tags\": [\"a\"] } });"
    );
    let found = generate(&editing, Generate::Select, &[row()]).unwrap();
    assert_eq!(found, "db.getCollection(\"users\").find({ \"_id\": ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\") });");
}

#[test]
fn a_row_reads_as_a_document_without_its_absent_fields() {
    let mut values = row();
    values[2] = Value::Null;
    values[4] = Value::Json("{\"a\":[1,2]}".into());
    assert_eq!(
        shell_document(&columns(), &values),
        "{\n  \"_id\": ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\"),\n  \"name\": \"Ada\",\n  \"views\": NumberLong(\"7\"),\n  \
         \"tags\": {\n    \"a\": [\n      1,\n      2\n    ]\n  }\n}"
    );
}
