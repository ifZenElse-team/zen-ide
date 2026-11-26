use super::*;

fn point_wkb(little: bool, kind: u32, coords: &[f64]) -> Vec<u8> {
    let mut out = vec![u8::from(little)];
    out.extend(if little { kind.to_le_bytes() } else { kind.to_be_bytes() });
    for value in coords {
        out.extend(if little { value.to_le_bytes() } else { value.to_be_bytes() });
    }
    out
}

fn polygon_wkb(srid: Option<u32>, rings: &[&[[f64; 2]]]) -> Vec<u8> {
    let mut out = vec![1u8];
    let kind = 3u32 | if srid.is_some() { 0x2000_0000 } else { 0 };
    out.extend(kind.to_le_bytes());
    if let Some(srid) = srid {
        out.extend(srid.to_le_bytes());
    }
    out.extend((rings.len() as u32).to_le_bytes());
    for ring in rings {
        out.extend((ring.len() as u32).to_le_bytes());
        for [x, y] in *ring {
            out.extend(x.to_le_bytes());
            out.extend(y.to_le_bytes());
        }
    }
    out
}

const SQUARE: [[f64; 2]; 5] = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]];
const HOLE: [[f64; 2]; 5] = [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [1.0, 1.0]];

#[test]
fn wkb_in_both_byte_orders_and_iso_dimensions() {
    let little = from_wkb(&point_wkb(true, 1, &[30.5, -10.0])).unwrap();
    let big = from_wkb(&point_wkb(false, 1, &[30.5, -10.0])).unwrap();
    assert_eq!(little.geometry, big.geometry);
    assert_eq!((little.encoding, little.srid), (Encoding::Wkb, None));
    assert_eq!(little.geometry.wkt(100).0, "POINT (30.5 -10)");
    let z = from_wkb(&point_wkb(true, 1001, &[1.0, 2.0, 3.0])).unwrap();
    assert_eq!(z.geometry.wkt(100).0, "POINT Z (1 2 3)");
    let zm = from_wkb(&point_wkb(true, 3001, &[1.0, 2.0, 3.0, 4.0])).unwrap();
    assert_eq!(zm.summary().dimensions, "xyzm");
    let empty = from_wkb(&point_wkb(true, 1, &[f64::NAN, f64::NAN])).unwrap();
    assert!(empty.geometry.is_empty());
    assert_eq!(empty.geometry.wkt(100).0, "POINT EMPTY");
    assert!(from_wkb(&point_wkb(true, 1, &[1.0])).is_none(), "too short");
    let mut longer = point_wkb(true, 1, &[1.0, 2.0]);
    longer.push(0);
    assert!(from_wkb(&longer).is_none(), "bytes left over");
    assert!(from_wkb(&point_wkb(true, 99, &[1.0, 2.0])).is_none(), "unknown kind");
}

#[test]
fn postgis_ewkb_keeps_its_srid_and_reads_from_hex() {
    let bytes = polygon_wkb(Some(4326), &[&SQUARE, &HOLE]);
    let spatial = from_bytes(&bytes).unwrap();
    assert_eq!((spatial.encoding, spatial.srid), (Encoding::Ewkb, Some(4326)));
    assert_eq!(spatial.ewkt(1000).0, "SRID=4326;POLYGON ((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))");
    let hex = format!("\\x{}", crate::value::hex(&bytes));
    let typed = Value::Other(Typed { text: hex.clone(), type_name: "geometry".into() });
    assert_eq!(from_value(&typed).unwrap(), spatial);
    assert_eq!(from_text(&hex[2..].to_ascii_uppercase()).unwrap(), spatial, "as ST_AsEWKB prints it");
    let summary = spatial.summary();
    assert_eq!((summary.kind.as_str(), summary.points, summary.parts), ("POLYGON", 10, 2));
    assert_eq!(summary.bounds, Some(Bounds { min_x: 0.0, min_y: 0.0, max_x: 4.0, max_y: 4.0 }));
    let ewkb_z = from_wkb(&point_wkb(true, 0x8000_0001, &[1.0, 2.0, 3.0])).unwrap();
    assert_eq!((ewkb_z.encoding, ewkb_z.geometry.z), (Encoding::Ewkb, true));
}

#[test]
fn mysql_and_geopackage_wrap_wkb_with_an_srid() {
    let mut mysql = 3857u32.to_le_bytes().to_vec();
    mysql.extend(point_wkb(true, 1, &[1.0, 2.0]));
    let read = from_bytes(&mysql).unwrap();
    assert_eq!((read.encoding, read.srid), (Encoding::Mysql, Some(3857)));
    let mut zero = 0u32.to_le_bytes().to_vec();
    zero.extend(point_wkb(true, 1, &[1.0, 2.0]));
    assert_eq!(from_bytes(&zero).unwrap().encoding, Encoding::Mysql, "SRID 0 starts like big-endian WKB");
    let mut gpkg = b"GP\x00\x03".to_vec();
    gpkg.extend(4326i32.to_le_bytes());
    gpkg.extend([1.0f64, 2.0, 1.0, 2.0].iter().flat_map(|value| value.to_le_bytes()));
    gpkg.extend(point_wkb(true, 1, &[1.0, 2.0]));
    let read = from_bytes(&gpkg).unwrap();
    assert_eq!(
        (read.encoding, read.srid, read.geometry.wkt(50).0.as_str()),
        (Encoding::Geopackage, Some(4326), "POINT (1 2)")
    );
    assert!(from_bytes(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").is_none());
}

#[test]
fn wkt_and_ewkt_of_every_kind_read_and_write_back() {
    for text in [
        "POINT (1 2)",
        "POINT Z (1 2 3)",
        "POINT M (1 2 7)",
        "POINT ZM (1 2 3 4)",
        "LINESTRING (0 0,1 1,2 0)",
        "LINESTRING EMPTY",
        "POLYGON ((0 0,4 0,4 4,0 0),(1 1,2 1,1 2,1 1))",
        "MULTIPOINT ((1 2),(3 4))",
        "MULTILINESTRING ((0 0,1 1),(2 2,3 3))",
        "MULTIPOLYGON (((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))",
        "GEOMETRYCOLLECTION (POINT (1 2),LINESTRING (0 0,1 1))",
        "GEOMETRYCOLLECTION EMPTY",
        "CIRCULARSTRING (0 0,1 1,2 0)",
        "COMPOUNDCURVE ((0 0,1 0),CIRCULARSTRING (1 0,2 1,3 0))",
        "CURVEPOLYGON (CIRCULARSTRING (0 0,2 0,0 0),(0.5 0.5,1 0.5,1 1,0.5 0.5))",
        "MULTISURFACE (CURVEPOLYGON (CIRCULARSTRING (0 0,2 0,0 0)),((5 5,6 5,6 6,5 5)))",
        "TIN (((0 0 0,1 0 0,0 1 0,0 0 0)))",
        "TRIANGLE ((0 0,1 0,0 1,0 0))",
    ] {
        let spatial = from_wkt(text).unwrap_or_else(|| panic!("{text}"));
        let expected = text.replace("TIN (", "TIN Z (");
        assert_eq!(spatial.geometry.wkt(1000).0, expected, "{text}");
    }
    let ewkt = from_wkt("SRID=4326;MULTIPOINT(1 2, 3 4)").unwrap();
    assert_eq!((ewkt.srid, ewkt.encoding), (Some(4326), Encoding::Ewkt));
    assert_eq!(ewkt.ewkt(100).0, "SRID=4326;MULTIPOINT ((1 2),(3 4))");
    assert_eq!(from_wkt("pointm(1 2 3)").unwrap().geometry.wkt(100).0, "POINT M (1 2 3)", "PostGIS's EWKT tags");
    assert_eq!(from_wkt("POINT(1 2 3)").unwrap().geometry.wkt(100).0, "POINT Z (1 2 3)");
    for broken in ["POINT (1)", "POINT (1 2", "POINT Z (1 2)", "BLOB (1 2)", "POINT (1 2) extra", "SRID=x;POINT(1 2)"] {
        assert!(from_wkt(broken).is_none(), "{broken}");
    }
    assert!(from_text("Customer 1").is_none());
    assert!(from_value(&Value::Text("POLYGON((0 0,1 0,1 1,0 0))".into())).is_some());
}

#[test]
fn geojson_geometries_features_and_collections() {
    let point = from_geojson(r#"{"type":"Point","coordinates":[30.5,50.25]}"#).unwrap();
    assert_eq!((point.encoding, point.geometry.wkt(100).0.as_str()), (Encoding::Geojson, "POINT (30.5 50.25)"));
    let feature = from_text(
        r#"{"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}}"#,
    )
    .unwrap();
    assert_eq!(feature.geometry.kind, Kind::Polygon);
    let collection = from_value(&Value::Json(
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2,3]}},{"type":"Feature","geometry":null}]}"#.into(),
    ))
    .unwrap();
    assert_eq!(collection.geometry.wkt(100).0, "GEOMETRYCOLLECTION Z (POINT Z (1 2 3))");
    assert!(from_geojson(r#"{"type":"Thing"}"#).is_none());
    assert!(from_text(r#"{"id":1}"#).is_none());
}

#[test]
fn wkt_is_cut_at_the_limit_and_its_whole_length_kept() {
    let line: Vec<String> = (0..1000).map(|at| format!("{at} {at}")).collect();
    let spatial = from_wkt(&format!("LINESTRING ({})", line.join(","))).unwrap();
    let (whole, length, cut) = spatial.geometry.wkt(usize::MAX);
    assert!(!cut);
    let (text, total, truncated) = spatial.geometry.wkt(40);
    assert_eq!((text.len(), total, truncated), (40, length, true));
    assert!(whole.starts_with(&text));
}

#[test]
fn outlines_split_points_lines_and_areas_and_cut_arcs() {
    let spatial = from_wkt(
        "GEOMETRYCOLLECTION (POINT (1 2),LINESTRING (0 0,1 1),POLYGON ((0 0,4 0,4 4,0 0),(1 1,2 1,1 2,1 1)),CIRCULARSTRING (0 0,1 1,2 0))",
    )
    .unwrap();
    let outlines = spatial.geometry.outlines();
    assert_eq!(outlines.points, [[1.0, 2.0]]);
    assert_eq!(outlines.lines.len(), 2);
    assert_eq!(outlines.areas.len(), 1);
    assert_eq!(outlines.areas[0].len(), 2, "with its hole");
    let arc = &outlines.lines[1];
    assert_eq!(arc.len(), ARC_STEPS + 1);
    assert_eq!(arc.last(), Some(&[2.0, 0.0]));
    assert!(arc.iter().all(|[x, y]| ((x - 1.0).hypot(*y) - 1.0).abs() < 1e-9), "on the circle");
    assert!(arc[ARC_STEPS / 2][1] > 0.99, "through the middle point");
    let circle = from_wkt("CURVEPOLYGON (CIRCULARSTRING (0 0,2 0,0 0))").unwrap().geometry.outlines();
    assert_eq!(circle.areas[0][0].len(), ARC_STEPS * 2 + 1, "a whole circle");
}

#[test]
fn hostile_input_is_refused_without_allocating() {
    let mut bytes = vec![1u8];
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(u32::MAX.to_le_bytes());
    assert!(from_wkb(&bytes).is_none(), "a count larger than the bytes");
    let mut nested = Vec::new();
    for _ in 0..100 {
        nested.push(1u8);
        nested.extend(7u32.to_le_bytes());
        nested.extend(1u32.to_le_bytes());
    }
    assert!(from_wkb(&nested).is_none(), "nested too deep");
    let deep = format!("{}POINT (1 2){}", "GEOMETRYCOLLECTION (".repeat(100), ")".repeat(100));
    assert!(from_wkt(&deep).is_none());
}

#[test]
fn values_as_mysql_8_4_stores_them_read_back() {
    let polygon = "E61000000103000000010000000400000000000000000000000000000000000000000000000000000000000000000010400000000000001040000000000000104000000000000000000000000000000000";
    let read = from_bytes(&unhex(polygon).unwrap()).unwrap();
    assert_eq!((read.encoding, read.srid), (Encoding::Mysql, Some(4326)));
    assert_eq!(read.geometry.wkt(100).0, "POLYGON ((0 0,0 4,4 4,0 0))", "latitude first, as MySQL keeps SRID 4326");
    let point = from_bytes(&unhex("000000000101000000000000000000F03F0000000000000040").unwrap()).unwrap();
    assert_eq!((point.srid, point.geometry.wkt(100).0.as_str()), (Some(0), "POINT (1 2)"));
}

#[test]
fn outline_points_bound_the_outlines_of_arcs_too() {
    for text in [
        "POINT(1 2)",
        "LINESTRING(0 0,1 1,2 0)",
        "CIRCULARSTRING(0 0,1 1,2 0,3 -1,4 0)",
        "CIRCULARSTRING(0 0,1 1,0 0)",
        "COMPOUNDCURVE(CIRCULARSTRING(0 0,1 1,2 0),(2 0,3 0))",
        "CURVEPOLYGON(CIRCULARSTRING(0 0,2 0,0 0))",
        "MULTIPOLYGON(((0 0,1 0,1 1,0 0)),((5 5,6 5,6 6,5 5)))",
    ] {
        let geometry = from_text(text).unwrap().geometry;
        let outlines = geometry.outlines();
        let drawn = outlines.points.len()
            + outlines.lines.iter().map(Vec::len).sum::<usize>()
            + outlines.areas.iter().flatten().map(Vec::len).sum::<usize>();
        assert!(drawn as u64 <= geometry.outline_points(), "{text}: {drawn} > {}", geometry.outline_points());
    }
}
