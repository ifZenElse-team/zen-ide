use mysql_async::consts::{ColumnFlags, ColumnType};

use crate::value::{Typed, Value};

const BINARY: u16 = 63;

pub(super) fn type_name(column: &mysql_async::Column) -> String {
    let flags = column.flags();
    let binary = column.character_set() == BINARY;
    let unsigned = |name: &str| {
        if flags.contains(ColumnFlags::UNSIGNED_FLAG) { format!("{name} unsigned") } else { name.to_owned() }
    };
    match column.column_type() {
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => unsigned("decimal"),
        ColumnType::MYSQL_TYPE_TINY => unsigned("tinyint"),
        ColumnType::MYSQL_TYPE_SHORT => unsigned("smallint"),
        ColumnType::MYSQL_TYPE_INT24 => unsigned("mediumint"),
        ColumnType::MYSQL_TYPE_LONG => unsigned("int"),
        ColumnType::MYSQL_TYPE_LONGLONG => unsigned("bigint"),
        ColumnType::MYSQL_TYPE_FLOAT => unsigned("float"),
        ColumnType::MYSQL_TYPE_DOUBLE => unsigned("double"),
        ColumnType::MYSQL_TYPE_NULL => "null".to_owned(),
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "timestamp".to_owned(),
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "date".to_owned(),
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "time".to_owned(),
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "datetime".to_owned(),
        ColumnType::MYSQL_TYPE_YEAR => "year".to_owned(),
        ColumnType::MYSQL_TYPE_BIT => "bit".to_owned(),
        ColumnType::MYSQL_TYPE_JSON => "json".to_owned(),
        ColumnType::MYSQL_TYPE_ENUM => "enum".to_owned(),
        ColumnType::MYSQL_TYPE_SET => "set".to_owned(),
        ColumnType::MYSQL_TYPE_GEOMETRY => "geometry".to_owned(),
        ColumnType::MYSQL_TYPE_VECTOR => "vector".to_owned(),
        ColumnType::MYSQL_TYPE_STRING if flags.contains(ColumnFlags::ENUM_FLAG) => "enum".to_owned(),
        ColumnType::MYSQL_TYPE_STRING if flags.contains(ColumnFlags::SET_FLAG) => "set".to_owned(),
        ColumnType::MYSQL_TYPE_STRING => if binary { "binary" } else { "char" }.to_owned(),
        ColumnType::MYSQL_TYPE_VARCHAR | ColumnType::MYSQL_TYPE_VAR_STRING => {
            if binary { "varbinary" } else { "varchar" }.to_owned()
        }
        ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_BLOB => if binary { "blob" } else { "text" }.to_owned(),
        _ => "unknown".to_owned(),
    }
}

pub(super) fn decode(column: &mysql_async::Column, type_name: &str, raw: Option<&[u8]>) -> Value {
    let Some(raw) = raw else { return Value::Null };
    let text = || String::from_utf8_lossy(raw).into_owned();
    let base = type_name.strip_suffix(" unsigned").unwrap_or(type_name);
    match base {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" => {
            let text = text();
            text.parse::<i64>().map_or(Value::Decimal(text), Value::Integer)
        }
        "decimal" => Value::Decimal(text()),
        "float" | "double" => {
            let text = text();
            text.parse::<f64>().map_or(Value::Decimal(text), Value::Float)
        }
        "timestamp" | "date" | "time" | "datetime" | "year" => {
            Value::Temporal(Typed { text: text(), type_name: base.to_owned() })
        }
        "bit" => bits(column, raw),
        "json" => Value::Json(text()),
        "binary" | "varbinary" | "blob" | "geometry" | "vector" => Value::Bytes(raw.to_vec()),
        _ => match std::str::from_utf8(raw) {
            Ok(text) => Value::Text(text.to_owned()),
            Err(_) => Value::Bytes(raw.to_vec()),
        },
    }
}

fn bits(column: &mysql_async::Column, raw: &[u8]) -> Value {
    let number = raw.iter().fold(0_u128, |number, byte| (number << 8) | u128::from(*byte));
    if column.column_length() == 1 {
        return Value::Bool(number != 0);
    }
    i64::try_from(number).map_or_else(|_| Value::Decimal(number.to_string()), Value::Integer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(kind: ColumnType, flags: ColumnFlags, charset: u16, length: u32) -> mysql_async::Column {
        mysql_async::Column::new(kind).with_flags(flags).with_character_set(charset).with_column_length(length)
    }

    fn value(column: &mysql_async::Column, raw: &[u8]) -> Value {
        decode(column, &type_name(column), Some(raw))
    }

    #[test]
    fn text_cells_become_typed_values() {
        let none = ColumnFlags::empty();
        let unsigned = column(ColumnType::MYSQL_TYPE_LONGLONG, ColumnFlags::UNSIGNED_FLAG, BINARY, 20);
        assert_eq!(type_name(&unsigned), "bigint unsigned");
        assert_eq!(value(&unsigned, b"18446744073709551615"), Value::Decimal("18446744073709551615".into()));
        assert_eq!(value(&unsigned, b"7"), Value::Integer(7));
        let decimal = column(ColumnType::MYSQL_TYPE_NEWDECIMAL, none, BINARY, 12);
        assert_eq!(value(&decimal, b"12.340"), Value::Decimal("12.340".into()));
        let date = column(ColumnType::MYSQL_TYPE_DATE, none, BINARY, 10);
        assert_eq!(
            value(&date, b"0000-00-00"),
            Value::Temporal(Typed { text: "0000-00-00".into(), type_name: "date".into() })
        );
        let flag = column(ColumnType::MYSQL_TYPE_BIT, ColumnFlags::UNSIGNED_FLAG, BINARY, 1);
        assert_eq!(value(&flag, &[1]), Value::Bool(true));
        let bits = column(ColumnType::MYSQL_TYPE_BIT, ColumnFlags::UNSIGNED_FLAG, BINARY, 10);
        assert_eq!(value(&bits, &[0x02, 0x01]), Value::Integer(513));
        let json = column(ColumnType::MYSQL_TYPE_JSON, ColumnFlags::BINARY_FLAG, BINARY, 0);
        assert_eq!(value(&json, b"{\"a\": 1}"), Value::Json("{\"a\": 1}".into()));
        let choice = column(ColumnType::MYSQL_TYPE_STRING, ColumnFlags::ENUM_FLAG, 255, 4);
        assert_eq!((type_name(&choice).as_str(), value(&choice, b"paid")), ("enum", Value::Text("paid".into())));
        let blob = column(ColumnType::MYSQL_TYPE_BLOB, ColumnFlags::BINARY_FLAG, BINARY, 65535);
        assert_eq!(value(&blob, &[0xde, 0xad]), Value::Bytes(vec![0xde, 0xad]));
        let text = column(ColumnType::MYSQL_TYPE_BLOB, none, 255, 65535);
        assert_eq!((type_name(&text).as_str(), value(&text, b"x")), ("text", Value::Text("x".into())));
        assert_eq!(decode(&text, "text", None), Value::Null);
    }
}
