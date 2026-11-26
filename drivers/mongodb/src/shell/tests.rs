use super::*;

fn run(statement: &str) -> Run {
    match parse(statement).unwrap() {
        Statement::Run(run) => run,
        other => panic!("{other:?}"),
    }
}

#[test]
fn values_are_read_as_the_shell_writes_them() {
    let value = parse_value(
        "{ name: 'Ada', \"age\": 36, big: 3000000000, ratio: -1.5e3, ok: true, none: null, $set: 1, 'a.b': [1, 2,], \
         id: ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\"), at: ISODate('2026-10-27T10:00:00Z'), day: new Date('2026-10-27'), \
         long: NumberLong('5'), dec: NumberDecimal('1.50'), re: /^a[/]b/i, hex: 0x1F, }",
    )
    .unwrap();
    let Bson::Document(document) = value else { panic!() };
    assert_eq!(document.get_str("name").unwrap(), "Ada");
    assert_eq!(document.get("age"), Some(&Bson::Int32(36)));
    assert_eq!(document.get("big"), Some(&Bson::Int64(3_000_000_000)));
    assert_eq!(document.get("ratio"), Some(&Bson::Double(-1500.0)));
    assert_eq!(document.get("a.b"), Some(&Bson::Array(vec![Bson::Int32(1), Bson::Int32(2)])));
    assert_eq!(document.get_object_id("id").unwrap().to_hex(), "6ae0b2ad2ee72a7b6191f5a9");
    assert_eq!(document.get_datetime("at").unwrap().timestamp_millis(), 1_793_095_200_000);
    assert_eq!(document.get_datetime("day").unwrap().timestamp_millis(), 1_793_059_200_000);
    assert_eq!(document.get("long"), Some(&Bson::Int64(5)));
    assert_eq!(document.get("dec").map(ToString::to_string).as_deref(), Some("1.50"));
    assert_eq!(document.get("re").map(ToString::to_string).as_deref(), Some("/^a[/]b/i"));
    assert_eq!(document.get("hex"), Some(&Bson::Int32(31)));
    assert!(document.contains_key("$set") && document.contains_key("none"));
}

#[test]
fn extended_json_wrappers_are_read_as_the_values_they_stand_for() {
    let value = parse_value(
        "{ id: {\"$oid\": \"6ae0b2ad2ee72a7b6191f5a9\"}, at: {\"$date\": {\"$numberLong\": \"1793095200000\"}}, \
         n: {\"$numberLong\": \"7\"}, q: { $gt: 1 }, plain: { a: 1 } }",
    )
    .unwrap();
    let Bson::Document(document) = value else { panic!() };
    assert_eq!(document.get_object_id("id").unwrap().to_hex(), "6ae0b2ad2ee72a7b6191f5a9");
    assert_eq!(document.get_datetime("at").unwrap().timestamp_millis(), 1_793_095_200_000);
    assert_eq!(document.get("n"), Some(&Bson::Int64(7)));
    assert_eq!(document.get_document("q").unwrap().get("$gt"), Some(&Bson::Int32(1)));
    assert_eq!(document.get_document("plain").unwrap().get("a"), Some(&Bson::Int32(1)));
}

#[test]
fn errors_point_at_where_reading_stopped() {
    let error = parse("db.people.find({ age: })").unwrap_err();
    assert_eq!((error.kind(), error.data.as_ref().unwrap()["position"].as_u64()), (Some("invalid"), Some(23)));
    assert!(parse("SELECT 1").is_err());
    assert!(parse("db.people.find({}) extra").is_err());
    assert!(parse("db.people.frobnicate()").is_err());
    assert!(parse_value("'open").is_err());
}

#[test]
fn find_and_its_modifiers_become_one_find_command() {
    let found = run("db.people.find({ age: { $gt: 30 } }, { name: 1 }).sort({ age: -1 }).skip(2).limit(5);");
    assert_eq!(
        found.command,
        doc! { "find": "people", "filter": { "age": { "$gt": 30 } }, "projection": { "name": 1 }, "sort": { "age": -1 }, "skip": 2_i64, "limit": 5_i64 }
    );
    assert_eq!((found.shape, found.writes, found.limit), (Shape::Cursor, false, Some(5)));
    let one = run("db.getCollection('a.b').findOne()");
    assert_eq!(one.command.get_str("find").unwrap(), "a.b");
    assert_eq!(one.limit, Some(1));
    assert_eq!(run("db['x y'].find()").command.get_str("find").unwrap(), "x y");
    assert_eq!(run("db.system.users.find()").command.get_str("find").unwrap(), "system.users");
    let other = run("db.getSiblingDB('shop').orders.find().count()");
    assert_eq!(other.database.as_deref(), Some("shop"));
    assert_eq!((other.command, other.shape), (doc! { "count": "orders" }, Shape::Count));
}

#[test]
fn changes_are_commands_that_write() {
    let inserted = run("db.people.insertOne({ name: 'Ada' })");
    assert!(inserted.writes);
    let documents = inserted.command.get_array("documents").unwrap();
    assert!(documents[0].as_document().unwrap().get_object_id("_id").is_ok(), "an _id is given before sending");
    let updated = run("db.people.updateMany({}, { $inc: { age: 1 } }, { upsert: true })");
    let entry = updated.command.get_array("updates").unwrap()[0].as_document().unwrap().clone();
    assert_eq!(entry, doc! { "q": {}, "u": { "$inc": { "age": 1 } }, "multi": true, "upsert": true });
    assert!(parse("db.people.updateOne({}, { age: 1 })").is_err(), "an update needs operators");
    assert!(parse("db.people.replaceOne({}, { $set: { age: 1 } })").is_err());
    let deleted = run("db.people.deleteOne({ name: 'Ada' })");
    assert_eq!(deleted.command.get_array("deletes").unwrap()[0].as_document().unwrap().get_i32("limit").unwrap(), 1);
    let index = run("db.people.createIndex({ name: 1, age: -1 }, { unique: true })");
    assert_eq!(
        index.command,
        doc! { "createIndexes": "people", "indexes": [{ "key": { "name": 1, "age": -1 }, "name": "name_1_age_-1", "unique": true }] }
    );
    assert!(run("db.people.drop()").writes);
    let modified = run("db.people.findOneAndUpdate({ a: 1 }, { $set: { a: 2 } }, { returnDocument: 'after' })");
    assert!(modified.command.get_bool("new").unwrap());
}

#[test]
fn aggregations_with_out_or_merge_and_unknown_commands_write() {
    assert!(!run("db.people.aggregate([{ $match: { a: 1 } }])").writes);
    assert!(run("db.people.aggregate([{ $match: {} }, { $out: 'copy' }])").writes);
    assert!(!run("db.runCommand({ ping: 1 })").writes);
    assert!(run("db.runCommand({ dropDatabase: 1 })").writes);
    assert!(!run("{ find: 'people', filter: {} }").writes);
    assert!(run("db.adminCommand('shutdown')").writes);
    assert_eq!(run("db.adminCommand({ listDatabases: 1 })").database.as_deref(), Some("admin"));
    assert!(!run("db.people.find().explain()").writes);
    let counted = run("db.people.countDocuments({ a: 1 }, { limit: 5 })");
    assert_eq!(counted.shape, Shape::CountFromCursor);
    assert_eq!(counted.command.get_array("pipeline").unwrap().len(), 3);
}

#[test]
fn show_and_use_are_read() {
    assert_eq!(parse("use shop").unwrap(), Statement::Use("shop".into()));
    let databases = run("show dbs");
    assert_eq!((databases.shape, databases.database.as_deref()), (Shape::Databases, Some("admin")));
    assert_eq!(run("show collections").command.keys().next().map(String::as_str), Some("listCollections"));
    assert!(parse("show users").is_err());
}
