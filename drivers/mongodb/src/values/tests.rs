use bson::oid::ObjectId;
use bson::{Binary, DateTime, doc};

use super::*;

#[test]
fn top_level_fields_become_columns_in_order_with_id_first() {
    let documents =
        [doc! { "name": "Ada", "_id": 1, "age": 36 }, doc! { "_id": 2, "name": 5, "city": "NYC", "age": null }];
    let columns = Columns::of(&documents);
    let wire = columns.wire();
    let names: Vec<&str> = wire.iter().filter_map(|column| column["name"].as_str()).collect();
    assert_eq!(names, ["_id", "name", "age", "city"]);
    assert_eq!(wire[1]["type"], "mixed");
    assert_eq!(wire[2]["type"], "int", "nulls do not change a column's type");
    assert_eq!(columns.row(&documents[1])[2], value::null());
    assert_eq!(columns.row(&documents[0])[3], value::null(), "a missing field is null");
}

#[test]
fn bson_values_become_protocol_values_and_nested_ones_json() {
    let id = ObjectId::parse_str("6ae0b2ad2ee72a7b6191f5a9").unwrap();
    assert_eq!(cell(&Bson::ObjectId(id))["value"]["text"], "ObjectId(\"6ae0b2ad2ee72a7b6191f5a9\")");
    assert_eq!(cell(&Bson::Int64(5)), value::integer(5));
    assert_eq!(cell(&Bson::DateTime(DateTime::from_millis(0)))["value"]["text"], "1970-01-01T00:00:00Z");
    assert_eq!(
        cell(&Bson::Document(doc! { "a": [1, { "b": id }] }))["value"],
        "{\"a\":[1,{\"b\":{\"$oid\":\"6ae0b2ad2ee72a7b6191f5a9\"}}]}"
    );
    assert_eq!(cell(&Bson::Array(vec![]))["kind"], "json");
    let uuid = bson::Uuid::parse_str("6f9619ff-8b86-d011-b42d-00c04fc964ff").unwrap();
    assert_eq!(cell(&Bson::Binary(Binary::from(uuid)))["value"], "6f9619ff-8b86-d011-b42d-00c04fc964ff");
    assert_eq!(cell(&Bson::Decimal128("1.50".parse().unwrap())), value::decimal("1.50"));
    assert_eq!(cell(&Bson::MaxKey)["value"]["type"], "maxKey");
}

#[test]
fn replies_lose_the_fields_every_reply_carries() {
    let row = reply_row(doc! { "ok": 1.0, "n": 2, "$clusterTime": {}, "operationTime": 1 });
    assert_eq!(row, doc! { "n": 2 });
}
