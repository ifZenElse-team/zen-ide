use super::*;
use crate::value::Typed;

#[test]
fn json_is_pretty_printed_in_order_with_folds() {
    let (text, folds) = pretty_json(r#"{"b":1,"a":[1, 2,{"x":"a,b:{"}],"e":{},"n":1.50}"#);
    assert_eq!(
        text,
        "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2,\n    {\n      \"x\": \"a,b:{\"\n    }\n  ],\n  \"e\": {},\n  \"n\": 1.50\n}"
    );
    assert_eq!(folds, [(0, 11), (2, 8), (5, 7)]);
}

#[test]
fn documents_maps_and_arrays_become_json_and_arrays_read_as_literals() {
    let document = Value::Document(vec![
        ("name".into(), Value::Text("Ann \"A\"".into())),
        ("tags".into(), Value::Array(vec![Value::Integer(1), Value::Null])),
        ("raw".into(), Value::Json("{\"k\":true}".into())),
    ]);
    assert_eq!(json_text(&document), r#"{"name":"Ann \"A\"","tags":[1,null],"raw":{"k":true}}"#);
    assert_eq!(formats(&document), [ValueFormat::Json, ValueFormat::Text]);
    let array = Value::Array(vec![
        Value::Integer(1),
        Value::Text("a b".into()),
        Value::Null,
        Value::Text("x\"y".into()),
        Value::Array(vec![Value::Text(String::new())]),
    ]);
    assert_eq!(plain_text(&array), r#"{1,"a b",NULL,"x\"y",{""}}"#);
    assert_eq!(chosen(&array, None), ValueFormat::Text);
    assert_eq!(chosen(&array, Some(ValueFormat::Json)), ValueFormat::Json);
    assert_eq!(chosen(&array, Some(ValueFormat::Image)), ValueFormat::Text);
}

#[test]
fn xml_is_indented_and_text_offers_json_and_xml_when_it_holds_them() {
    let xml = "<?xml version=\"1.0\"?><a x=\"1\"><b>text</b><c/><d><e></e></d><!-- note --></a>";
    let (text, folds) = pretty_xml(xml).unwrap();
    assert_eq!(
        text,
        "<?xml version=\"1.0\"?>\n<a x=\"1\">\n  <b>text</b>\n  <c/>\n  <d>\n    <e></e>\n  </d>\n  <!-- note -->\n</a>"
    );
    assert_eq!(folds, [(1, 8), (4, 6)]);
    assert!(pretty_xml("<a><b></a>").is_none(), "badly nested");
    let value = Value::Text(xml.into());
    assert_eq!(formats(&value), [ValueFormat::Xml, ValueFormat::Text, ValueFormat::Hex]);
    let json = Value::Text("[1,2]".into());
    assert_eq!(formats(&json)[0], ValueFormat::Json);
    assert_eq!(formats(&Value::Text("{not json".into()))[0], ValueFormat::Text);
}

#[test]
fn bytes_show_as_hex_and_pictures_as_images() {
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR and more bytes".to_vec();
    let value = Value::Bytes(png.clone());
    assert_eq!(formats(&value)[..2], [ValueFormat::Image, ValueFormat::Hex]);
    assert_eq!(image_kind(&png), Some("PNG"));
    let dump = render(&value, ValueFormat::Hex, 10_000).unwrap();
    assert_eq!(
        dump.text.lines().next().unwrap(),
        "00000000  89 50 4e 47 0d 0a 1a 0a  00 00 00 0d 49 48 44 52  |.PNG........IHDR|"
    );
    assert!(!dump.truncated);
    let long = Value::Bytes(vec![7; 4096]);
    let cut = render(&long, ValueFormat::Hex, 200).unwrap();
    assert!(cut.truncated && cut.text.lines().count() <= 4, "{}", cut.text);
    assert_eq!(render(&value, ValueFormat::Image, 100).unwrap().text, format!("PNG, {} bytes", png.len()));
    assert!(render(&Value::Integer(1), ValueFormat::Hex, 10).is_none());
}

#[test]
fn long_text_is_cut_and_its_length_kept() {
    let value = Value::Text("é".repeat(5000));
    let rendered = render(&value, ValueFormat::Text, 100).unwrap();
    assert_eq!((rendered.text.chars().count(), rendered.length, rendered.truncated), (100, 5000, true));
    let stamp = Value::Temporal(Typed { text: "2026-10-26 21:42:01+08".into(), type_name: "timestamptz".into() });
    assert_eq!(render(&stamp, ValueFormat::Text, 100).unwrap().text, "2026-10-26 21:42:01+08", "as the server sent it");
}

#[test]
fn calc_counts_sums_and_compares() {
    let values =
        vec![Value::Integer(3), Value::Null, Value::Integer(1), Value::Integer(3), Value::Decimal("2.5".into())];
    let calc = calc(values, 100);
    assert_eq!((calc.cells, calc.count, calc.numeric, calc.distinct), (5, 4, 4, 3));
    assert_eq!(calc.sum.as_deref(), Some("9.5"));
    assert_eq!(calc.avg.as_deref(), Some("2.375"));
    assert_eq!((calc.min.as_deref(), calc.max.as_deref()), (Some("1"), Some("3")));
    let exact = super::calc(vec![Value::Integer(i64::MAX), Value::Integer(i64::MAX)], 100);
    assert_eq!(exact.sum.as_deref(), Some("18446744073709551614"), "integers sum exactly");
    let money = super::calc(
        vec![
            Value::Decimal("0.10".to_owned()),
            Value::Decimal("0.20".to_owned()),
            Value::Decimal("12345678901234567.89".to_owned()),
            Value::Integer(1),
            Value::Decimal("-0.05".to_owned()),
        ],
        100,
    );
    assert_eq!(money.sum.as_deref(), Some("12345678901234569.14"), "decimals sum exactly");
    let small = super::calc(vec![Value::Decimal("-0.005".to_owned()), Value::Decimal("0.001".to_owned())], 100);
    assert_eq!(small.sum.as_deref(), Some("-0.004"));
    let mixed = super::calc(vec![Value::Decimal("0.1".to_owned()), Value::Float(0.5)], 100);
    assert_eq!(mixed.sum.as_deref(), Some("0.6"), "a float makes it a floating-point sum");
    let words = super::calc(vec![Value::Text("pear".into()), Value::Text("apple".into())], 1);
    assert_eq!((words.min.as_deref(), words.sum.as_deref(), words.truncated), (Some("pear"), None, true));
}

#[test]
fn spatial_values_offer_the_geometry_view_and_render_as_wkt() {
    let ewkb = "0101000020E6100000000000000000F03F0000000000000040";
    let postgis =
        Value::Other(Typed { text: format!("\\x{}", ewkb.to_ascii_lowercase()), type_name: "geometry".into() });
    assert_eq!(formats(&postgis)[0], ValueFormat::Geometry);
    assert_eq!(render(&postgis, ValueFormat::Geometry, 100).unwrap().text, "SRID=4326;POINT (1 2)");
    let wkt = Value::Text("LINESTRING(0 0, 1 1)".into());
    assert_eq!(formats(&wkt), [ValueFormat::Geometry, ValueFormat::Text, ValueFormat::Hex]);
    let geojson = Value::Json(r#"{"type":"Point","coordinates":[1,2]}"#.into());
    assert_eq!(formats(&geojson), [ValueFormat::Json, ValueFormat::Geometry, ValueFormat::Text]);
    let cut = render(&wkt, ValueFormat::Geometry, 8).unwrap();
    assert_eq!((cut.text.as_str(), cut.length, cut.truncated), ("LINESTRI", 20, true));
    assert!(!formats(&Value::Text("Customer 1".into())).contains(&ValueFormat::Geometry));
    assert!(render(&Value::Integer(1), ValueFormat::Geometry, 10).is_none());
}

#[test]
fn text_or_json_never_offers_a_geometry() {
    let point = Value::Text("POINT(1 2)".to_owned());
    assert_eq!(formats(&point)[0], ValueFormat::Geometry);
    assert_eq!(text_or_json(&point), ValueFormat::Text);
    assert_eq!(text_or_json(&Value::Json("{\"a\":1}".to_owned())), ValueFormat::Json);
}
