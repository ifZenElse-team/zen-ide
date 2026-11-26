use serde_json::{Value as Json, json};
use zen_db_driver_kit::value;

use crate::json::Raw;

pub(crate) fn base(type_name: &str) -> String {
    let mut out = String::new();
    let mut depth = 0_u32;
    for c in type_name.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c.to_ascii_uppercase()),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn number(text: &str) -> Json {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && let Ok(integer) = text.parse::<i64>()
    {
        return value::integer(integer);
    }
    let fixed = if let Some(rest) = text.strip_prefix("-.") {
        format!("-0.{rest}")
    } else if let Some(rest) = text.strip_prefix('.') {
        format!("0.{rest}")
    } else {
        text.to_owned()
    };
    value::decimal(fixed)
}

fn float(text: &str) -> Json {
    match text.parse::<f64>() {
        Ok(parsed) if parsed.is_finite() => value::float(parsed),
        _ => value::other(text, "binary_double"),
    }
}

pub(crate) fn cell(raw: &Raw, type_name: &str) -> Json {
    let base = base(type_name);
    if let Raw::Array(_) | Raw::Object(_) = raw {
        return value::json_text(raw.to_json());
    }
    let Some(text) = raw.scalar() else {
        return value::null();
    };
    match base.as_str() {
        "NUMBER" | "INTEGER" | "INT" | "SMALLINT" | "DECIMAL" | "NUMERIC" | "FLOAT" | "BINARY_INTEGER"
        | "PLS_INTEGER" | "NATURAL" | "POSITIVE" | "SIMPLE_INTEGER" => number(&text),
        "BINARY_FLOAT" | "BINARY_DOUBLE" | "REAL" | "DOUBLE PRECISION" => float(&text),
        "DATE" => value::temporal(text, "date"),
        base if base.starts_with("TIMESTAMP") => value::temporal(text, &type_name.to_ascii_lowercase()),
        base if base.starts_with("INTERVAL") => json!({ "kind": "interval", "value": text }),
        "RAW" | "LONG RAW" | "BLOB" | "BFILE" => value::unhex(&text)
            .map_or_else(|| value::other(&text, &type_name.to_ascii_lowercase()), |bytes| value::bytes(&bytes)),
        "JSON" => value::json_text(text),
        "BOOLEAN" | "PL/SQL BOOLEAN" => match text.to_ascii_lowercase().as_str() {
            "true" | "1" => value::boolean(true),
            "false" | "0" => value::boolean(false),
            _ => value::text(text),
        },
        "VARCHAR2" | "VARCHAR" | "NVARCHAR2" | "CHAR" | "NCHAR" | "CLOB" | "NCLOB" | "LONG" | "ROWID" | "UROWID"
        | "" => match raw {
            Raw::Number(_) if base.is_empty() => number(&text),
            Raw::Bool(value) if base.is_empty() => value::boolean(*value),
            _ => value::text(text),
        },
        _ => match raw {
            Raw::Number(_) => number(&text),
            Raw::Bool(value) => value::boolean(*value),
            _ => value::other(&text, &type_name.to_ascii_lowercase()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Raw {
        Raw::Text(value.to_owned())
    }

    fn num(value: &str) -> Raw {
        Raw::Number(value.to_owned())
    }

    #[test]
    fn numbers_stay_exact() {
        assert_eq!(cell(&num("42"), "NUMBER"), value::integer(42));
        assert_eq!(cell(&num("-7"), "NUMBER(10)"), value::integer(-7));
        assert_eq!(
            cell(&num("123456789012345678901234567890"), "NUMBER"),
            value::decimal("123456789012345678901234567890")
        );
        assert_eq!(cell(&num(".5"), "NUMBER"), value::decimal("0.5"));
        assert_eq!(cell(&num("-.5"), "NUMBER"), value::decimal("-0.5"));
        assert_eq!(cell(&num("1.5E+20"), "NUMBER"), value::decimal("1.5E+20"));
        assert_eq!(cell(&num("2.5"), "BINARY_DOUBLE"), value::float(2.5));
        assert_eq!(cell(&text("Nan"), "BINARY_DOUBLE")["kind"], "other");
    }

    #[test]
    fn texts_dates_bytes_and_nulls_by_their_type() {
        assert_eq!(cell(&text(""), "VARCHAR2"), value::null());
        assert_eq!(cell(&Raw::Null, "NUMBER"), value::null());
        assert_eq!(cell(&text("Ada"), "VARCHAR2"), value::text("Ada"));
        assert_eq!(cell(&text("2026-10-31 10:00:00"), "DATE"), value::temporal("2026-10-31 10:00:00", "date"));
        assert_eq!(
            cell(&text("2026-10-31 10:00:00.5 +03:00"), "TIMESTAMP(6) WITH TIME ZONE"),
            value::temporal("2026-10-31 10:00:00.5 +03:00", "timestamp(6) with time zone")
        );
        assert_eq!(cell(&text("+02 03:00:00.000000"), "INTERVAL DAY(2) TO SECOND(6)")["kind"], "interval");
        assert_eq!(cell(&text("00FF"), "RAW"), value::bytes(&[0, 255]));
        assert_eq!(cell(&text("TRUE"), "BOOLEAN"), value::boolean(true));
        assert_eq!(cell(&Raw::Array(vec![num("1")]), "JSON"), value::json_text("[1]"));
        assert_eq!(cell(&text("<a/>"), "XMLTYPE"), value::other("<a/>", "xmltype"));
        assert_eq!(cell(&num("3"), ""), value::integer(3));
    }

    #[test]
    fn precision_is_left_out_of_the_base_type() {
        assert_eq!(base("timestamp(6) with  time zone"), "TIMESTAMP WITH TIME ZONE");
        assert_eq!(base("NUMBER(10,2)"), "NUMBER");
    }
}
