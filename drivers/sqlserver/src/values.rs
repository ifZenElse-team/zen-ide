use serde_json::Value as Json;
use tiberius::numeric::Numeric;
use tiberius::time::{Date, DateTime, DateTime2, DateTimeOffset, SmallDateTime, Time};
use tiberius::{ColumnData, ColumnType};
use zen_db_driver_kit::value;

const DAYS_TO_1900: i64 = 25_567;
const DAYS_TO_0001: i64 = 719_162;

pub(crate) fn wire(data: &ColumnData<'_>) -> Json {
    match data {
        ColumnData::U8(value) => value.map_or_else(value::null, |v| value::integer(i64::from(v))),
        ColumnData::I16(value) => value.map_or_else(value::null, |v| value::integer(i64::from(v))),
        ColumnData::I32(value) => value.map_or_else(value::null, |v| value::integer(i64::from(v))),
        ColumnData::I64(value) => value.map_or_else(value::null, value::integer),
        ColumnData::F32(value) => value.map_or_else(value::null, |v| value::float(f64::from(v))),
        ColumnData::F64(value) => value.map_or_else(value::null, value::float),
        ColumnData::Bit(value) => value.map_or_else(value::null, value::boolean),
        ColumnData::String(value) => value.as_ref().map_or_else(value::null, |v| value::text(v.as_ref())),
        ColumnData::Guid(value) => value.map_or_else(value::null, |v| value::uuid(v.to_string())),
        ColumnData::Binary(value) => value.as_ref().map_or_else(value::null, |v| value::bytes(v)),
        ColumnData::Numeric(value) => value.map_or_else(value::null, |v| value::decimal(decimal(v))),
        ColumnData::Xml(value) => {
            value.as_ref().map_or_else(value::null, |v| value::other(&v.as_ref().clone().into_string(), "xml"))
        }
        ColumnData::DateTime(value) => value.map_or_else(value::null, |v| value::temporal(datetime(v), "datetime")),
        ColumnData::SmallDateTime(value) => {
            value.map_or_else(value::null, |v| value::temporal(small_datetime(v), "smalldatetime"))
        }
        ColumnData::Time(value) => value.map_or_else(value::null, |v| value::temporal(time(v), "time")),
        ColumnData::Date(value) => value.map_or_else(value::null, |v| value::temporal(date(v), "date")),
        ColumnData::DateTime2(value) => value.map_or_else(value::null, |v| value::temporal(datetime2(v), "datetime2")),
        ColumnData::DateTimeOffset(value) => {
            value.map_or_else(value::null, |v| value::temporal(datetime_offset(v), "datetimeoffset"))
        }
    }
}

pub(crate) fn decimal(number: Numeric) -> String {
    let digits = number.value().unsigned_abs().to_string();
    let scale = usize::from(number.scale());
    let sign = if number.value() < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

fn nanos(time: Time) -> u64 {
    let scale = u32::from(time.scale().min(9));
    time.increments().saturating_mul(10u64.pow(9 - scale))
}

pub(crate) fn time(time: Time) -> String {
    value::time_text(nanos(time), time.scale())
}

pub(crate) fn date(date: Date) -> String {
    value::date_text(i64::from(date.days()) - DAYS_TO_0001)
}

pub(crate) fn datetime(value: DateTime) -> String {
    let millis = (u64::from(value.seconds_fragments()) * 1000 + 150) / 300;
    let days = i64::from(value.days()) - DAYS_TO_1900 + i64::try_from(millis / 86_400_000).unwrap_or(0);
    let millis = millis % 86_400_000;
    format!("{} {}", value::date_text(days), value::time_text(millis * 1_000_000, 3))
}

pub(crate) fn small_datetime(value: SmallDateTime) -> String {
    let days = i64::from(value.days()) - DAYS_TO_1900;
    let nanos = u64::from(value.seconds_fragments()) * 60 * 1_000_000_000;
    format!("{} {}", value::date_text(days), value::time_text(nanos, 0))
}

pub(crate) fn datetime2(value: DateTime2) -> String {
    format!("{} {}", date(value.date()), time(value.time()))
}

pub(crate) fn datetime_offset(value: DateTimeOffset) -> String {
    let utc = value.datetime2();
    let scale = utc.time().scale();
    let day_nanos: i128 = 86_400 * 1_000_000_000;
    let offset = i64::from(value.offset());
    let total = (i128::from(utc.date().days()) - i128::from(DAYS_TO_0001)) * day_nanos
        + i128::from(nanos(utc.time()))
        + i128::from(offset) * 60 * 1_000_000_000;
    let days = i64::try_from(total.div_euclid(day_nanos)).unwrap_or(0);
    let within = u64::try_from(total.rem_euclid(day_nanos)).unwrap_or(0);
    format!("{} {} {}", value::date_text(days), value::time_text(within, scale), value::offset_text(offset))
}

pub(crate) fn type_name(column: ColumnType) -> &'static str {
    match column {
        ColumnType::Null => "null",
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 | ColumnType::Intn => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money => "money",
        ColumnType::Money4 => "smallmoney",
        ColumnType::Datetime | ColumnType::Datetimen => "datetime",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigBinary => "binary",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::Text => "text",
        ColumnType::Image => "image",
        ColumnType::NText => "ntext",
        ColumnType::SSVariant => "sql_variant",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_keep_their_scale_and_sign() {
        assert_eq!(decimal(Numeric::new_with_scale(1250, 2)), "12.50");
        assert_eq!(decimal(Numeric::new_with_scale(-5, 1)), "-0.5");
        assert_eq!(decimal(Numeric::new_with_scale(7, 3)), "0.007");
        assert_eq!(decimal(Numeric::new_with_scale(42, 0)), "42");
    }

    #[test]
    fn dates_and_times_read_as_sql_server_writes_them() {
        assert_eq!(date(Date::new(738_885)), "2024-01-01");
        assert_eq!(time(Time::new(123_456_789, 7)), "00:00:12.3456789");
        assert_eq!(datetime2(DateTime2::new(Date::new(0), Time::new(0, 3))), "0001-01-01 00:00:00.000");
        assert_eq!(datetime(DateTime::new(45_290, 300 * 3600 + 1)), "2024-01-01 01:00:00.003");
        assert_eq!(datetime(DateTime::new(0, 300 * 86_400 - 1)), "1900-01-01 23:59:59.997");
        assert_eq!(small_datetime(SmallDateTime::new(45_290, 61)), "2024-01-01 01:01:00");
    }

    #[test]
    fn offsets_show_the_local_time() {
        let utc = DateTime2::new(Date::new(738_885), Time::new(0, 0));
        assert_eq!(datetime_offset(DateTimeOffset::new(utc, 120)), "2024-01-01 02:00:00 +02:00");
        let midnight = DateTime2::new(Date::new(738_885), Time::new(3600, 0));
        assert_eq!(datetime_offset(DateTimeOffset::new(midnight, -300)), "2023-12-31 20:00:00 -05:00");
    }

    #[test]
    fn cells_become_protocol_values() {
        assert_eq!(wire(&ColumnData::I32(Some(7)))["value"], 7);
        assert_eq!(wire(&ColumnData::I32(None))["kind"], "null");
        assert_eq!(wire(&ColumnData::Bit(Some(true)))["value"], true);
        assert_eq!(wire(&ColumnData::Binary(Some(vec![1, 255].into())))["value"], "01ff");
        assert_eq!(wire(&ColumnData::String(Some("Ada".into())))["value"], "Ada");
        assert_eq!(wire(&ColumnData::Numeric(Some(Numeric::new_with_scale(5, 1))))["value"], "0.5");
    }
}
