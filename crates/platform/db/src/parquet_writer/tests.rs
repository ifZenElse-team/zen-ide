use arrow_array::cast::AsArray;
use arrow_array::types::{
    Date32Type, Decimal128Type, Float64Type, Int16Type, Int32Type, Int64Type, Time64MicrosecondType,
    TimestampMicrosecondType,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use super::*;
use crate::value::Typed;

fn columns() -> Vec<Column> {
    [
        ("id", "int8"),
        ("small", "int2"),
        ("count", "int4"),
        ("ratio", "float8"),
        ("price", "numeric(10,2)"),
        ("loose", "numeric"),
        ("active", "bool"),
        ("name", "text"),
        ("picture", "bytea"),
        ("born", "date"),
        ("at", "time"),
        ("seen", "timestamp"),
        ("moment", "timestamptz"),
        ("tags", "_text"),
    ]
    .iter()
    .map(|(name, kind)| Column::new(*name, *kind))
    .collect()
}

fn temporal(text: &str, kind: &str) -> Value {
    Value::Temporal(Typed { text: text.to_owned(), type_name: kind.to_owned() })
}

fn row(id: i64) -> Vec<Value> {
    vec![
        Value::Integer(id),
        Value::Integer(-3),
        Value::Integer(7),
        Value::Float(0.5),
        Value::Decimal("12.3".to_owned()),
        Value::Decimal("1.000001".to_owned()),
        Value::Bool(id % 2 == 0),
        Value::Text(format!("row {id}")),
        Value::Bytes(vec![0, 1, 255]),
        temporal("2026-10-27", "date"),
        temporal("09:30:00.5", "time"),
        temporal("2026-10-27 09:30:00", "timestamp"),
        temporal("2026-10-27 09:30:00+03", "timestamptz"),
        Value::Array(vec![Value::Text("a".to_owned())]),
    ]
}

fn written(compression: Compression, rows: i64) -> Vec<u8> {
    let settings = Settings { compression, row_group: 1_000, ..Settings::default() };
    let mut writer = ParquetWriter::new(Vec::new(), &settings, columns()).unwrap();
    for id in 0..rows {
        writer.row(&row(id)).unwrap();
    }
    writer.row(&[Value::Integer(rows)]).unwrap();
    assert_eq!(writer.rows(), u64::try_from(rows).unwrap() + 1);
    writer.finish().unwrap()
}

#[test]
fn rows_are_written_as_typed_columns_and_read_back() {
    let file = written(Compression::Snappy, 5000);
    let reader = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(file)).unwrap();
    assert_eq!(reader.metadata().num_row_groups(), 6, "row groups of 1 000 rows");
    let schema = reader.schema().clone();
    let types: Vec<String> = schema.fields().iter().map(|field| field.data_type().to_string()).collect();
    assert_eq!(
        types,
        [
            "Int64",
            "Int16",
            "Int32",
            "Float64",
            "Decimal128(10, 2)",
            "Utf8",
            "Boolean",
            "Utf8",
            "Binary",
            "Date32",
            "Time64(µs)",
            "Timestamp(µs)",
            "Timestamp(µs, \"UTC\")",
            "Utf8"
        ]
    );
    let batches: Vec<RecordBatch> = reader.build().unwrap().map(Result::unwrap).collect();
    assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 5001);
    let first = &batches[0];
    assert_eq!(first.column(0).as_primitive::<Int64Type>().value(1), 1);
    assert_eq!(first.column(1).as_primitive::<Int16Type>().value(0), -3);
    assert_eq!(first.column(2).as_primitive::<Int32Type>().value(0), 7);
    assert_eq!(first.column(3).as_primitive::<Float64Type>().value(0), 0.5);
    assert_eq!(first.column(4).as_primitive::<Decimal128Type>().value(0), 1230);
    assert_eq!(first.column(5).as_string::<i32>().value(0), "1.000001");
    assert!(first.column(6).as_boolean().value(0));
    assert_eq!(first.column(7).as_string::<i32>().value(2), "row 2");
    assert_eq!(first.column(8).as_binary::<i32>().value(0), [0, 1, 255]);
    assert_eq!(first.column(9).as_primitive::<Date32Type>().value(0), 20_753);
    assert_eq!(first.column(10).as_primitive::<Time64MicrosecondType>().value(0), 34_200_500_000);
    let seen = first.column(11).as_primitive::<TimestampMicrosecondType>().value(0);
    let moment = first.column(12).as_primitive::<TimestampMicrosecondType>().value(0);
    assert_eq!(seen - moment, 3 * 3_600 * 1_000_000, "the offset is taken out");
    assert_eq!(first.column(13).as_string::<i32>().value(0), "[\"a\"]");
    let last = batches.last().unwrap();
    let at = last.num_rows() - 1;
    assert_eq!(last.column(0).as_primitive::<Int64Type>().value(at), 5000);
    assert!(last.column(7).is_null(at), "missing values are NULL");
}

#[test]
fn every_codec_writes_a_file_readers_take() {
    for compression in Compression::ALL {
        let file = written(compression, 10);
        let reader = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(file)).unwrap();
        let rows: usize = reader.build().unwrap().map(|batch| batch.unwrap().num_rows()).sum();
        assert_eq!(rows, 11, "{compression:?}");
    }
}

#[test]
fn a_value_its_column_cannot_hold_fails_with_the_column() {
    let mut writer = ParquetWriter::new(Vec::new(), &Settings::default(), vec![Column::new("n", "int4")]).unwrap();
    let error = writer.row(&[Value::Text("seven".to_owned())]).unwrap_err();
    assert!(error.to_string().contains("column n"), "{error}");
}

#[test]
fn decimals_are_read_to_their_scale_exactly() {
    assert_eq!(decimal("12.3", 2), Some(1230));
    assert_eq!(decimal("-0.05", 2), Some(-5));
    assert_eq!(decimal("+7", 0), Some(7));
    assert_eq!(decimal("1.50", 1), Some(15));
    assert_eq!(decimal("1.234", 2), None);
    assert_eq!(decimal("NaN", 2), None);
    assert_eq!(decimal("", 2), None);
}
