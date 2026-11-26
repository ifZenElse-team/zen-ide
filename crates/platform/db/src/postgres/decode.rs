use std::fmt::Write;

use fallible_iterator::FallibleIterator;

use postgres_protocol::types as wire;
use tokio_postgres::types::{FromSql, Kind, Type};

use crate::value::{Typed, Value};

pub(super) struct Raw<'a>(pub(super) &'a [u8]);

impl<'a> FromSql<'a> for Raw<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Raw(raw))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

const USECS_PER_DAY: i64 = 86_400_000_000;
const DAYS_2000_FROM_1970: i64 = 10_957;

#[derive(Clone, Debug)]
pub(super) struct Zone {
    name: String,
    zone: Option<jiff::tz::TimeZone>,
}

impl Zone {
    pub(super) fn named(name: &str) -> Self {
        let zone = jiff::tz::TimeZone::get(name).ok().or_else(|| jiff::tz::TimeZone::posix(name).ok());
        Self { name: name.to_owned(), zone }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    fn offset_at(&self, micros_since_2000: i64) -> i32 {
        let Some(zone) = &self.zone else { return 0 };
        let unix = micros_since_2000.saturating_add(DAYS_2000_FROM_1970 * USECS_PER_DAY);
        jiff::Timestamp::from_microsecond(unix).map_or(0, |instant| zone.to_offset(instant).seconds())
    }
}

pub(super) fn decode(ty: &Type, raw: Option<&[u8]>, zone: &Zone) -> Value {
    let Some(raw) = raw else { return Value::Null };
    decoded(ty, raw, zone).unwrap_or_else(|| {
        Value::Other(Typed { text: format!("\\x{}", crate::value::hex(raw)), type_name: ty.name().to_owned() })
    })
}

fn decoded(ty: &Type, raw: &[u8], zone: &Zone) -> Option<Value> {
    let temporal = |text: String| Some(Value::Temporal(Typed { text, type_name: ty.name().to_owned() }));
    let other = |text: String| Some(Value::Other(Typed { text, type_name: ty.name().to_owned() }));
    match ty.kind() {
        Kind::Array(element) => return array(element, raw, zone),
        Kind::Domain(base) => return decoded(base, raw, zone),
        Kind::Enum(_) => return Some(Value::Text(String::from_utf8_lossy(raw).into_owned())),
        _ => {}
    }
    match *ty {
        Type::BOOL => Some(Value::Bool(wire::bool_from_sql(raw).ok()?)),
        Type::INT2 => Some(Value::Integer(wire::int2_from_sql(raw).ok()?.into())),
        Type::INT4 => Some(Value::Integer(wire::int4_from_sql(raw).ok()?.into())),
        Type::INT8 => Some(Value::Integer(wire::int8_from_sql(raw).ok()?)),
        Type::OID => Some(Value::Integer(wire::oid_from_sql(raw).ok()?.into())),
        Type::FLOAT4 => Some(Value::Float(wire::float4_from_sql(raw).ok()?.into())),
        Type::FLOAT8 => Some(Value::Float(wire::float8_from_sql(raw).ok()?)),
        Type::NUMERIC => Some(Value::Decimal(numeric(raw)?)),
        Type::MONEY => {
            let cents = wire::int8_from_sql(raw).ok()?;
            Some(Value::Decimal(format!(
                "{}{}.{:02}",
                if cents < 0 { "-" } else { "" },
                (cents / 100).unsigned_abs(),
                (cents % 100).unsigned_abs()
            )))
        }
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN | Type::XML | Type::REGCLASS => {
            Some(Value::Text(std::str::from_utf8(raw).ok()?.to_owned()))
        }
        Type::CHAR => Some(Value::Text(char::from(*raw.first()?).to_string())),
        Type::BYTEA => Some(Value::Bytes(raw.to_vec())),
        Type::UUID => {
            let bytes = wire::uuid_from_sql(raw).ok()?;
            let hex = crate::value::hex(&bytes);
            Some(Value::Uuid(format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])))
        }
        Type::JSON => Some(Value::Json(std::str::from_utf8(raw).ok()?.to_owned())),
        Type::JSONB => {
            let (version, text) = raw.split_first()?;
            (*version == 1).then(|| Value::Json(String::from_utf8_lossy(text).into_owned()))
        }
        Type::DATE => temporal(date(wire::date_from_sql(raw).ok()?)),
        Type::TIME => temporal(time(wire::time_from_sql(raw).ok()?)),
        Type::TIMETZ => {
            let micros = i64::from_be_bytes(raw.get(..8)?.try_into().ok()?);
            let west = i32::from_be_bytes(raw.get(8..12)?.try_into().ok()?);
            temporal(format!("{}{}", time(micros), offset(-west)))
        }
        Type::TIMESTAMP => temporal(timestamp(wire::timestamp_from_sql(raw).ok()?, None)),
        Type::TIMESTAMPTZ => {
            let micros = wire::timestamp_from_sql(raw).ok()?;
            let east = if micros == i64::MAX || micros == i64::MIN { 0 } else { zone.offset_at(micros) };
            temporal(timestamp(micros, Some(east)))
        }
        Type::INTERVAL => {
            let micros = i64::from_be_bytes(raw.get(..8)?.try_into().ok()?);
            let days = i32::from_be_bytes(raw.get(8..12)?.try_into().ok()?);
            let months = i32::from_be_bytes(raw.get(12..16)?.try_into().ok()?);
            Some(Value::Interval(interval(months, days, micros)))
        }
        Type::INET | Type::CIDR => {
            let inet = wire::inet_from_sql(raw).ok()?;
            let full = if inet.addr().is_ipv4() { 32 } else { 128 };
            if *ty == Type::INET && inet.netmask() == full {
                other(inet.addr().to_string())
            } else {
                other(format!("{}/{}", inet.addr(), inet.netmask()))
            }
        }
        Type::MACADDR => {
            let bytes = wire::macaddr_from_sql(raw).ok()?;
            other(bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(":"))
        }
        Type::BIT | Type::VARBIT => {
            let bits = wire::varbit_from_sql(raw).ok()?;
            other(
                bits.bytes()
                    .iter()
                    .flat_map(|byte| (0..8).rev().map(move |at| if byte >> at & 1 == 1 { '1' } else { '0' }))
                    .take(bits.len())
                    .collect(),
            )
        }
        _ => None,
    }
}

fn array(element: &Type, raw: &[u8], zone: &Zone) -> Option<Value> {
    use postgres_protocol::types::ArrayDimension;
    let array = wire::array_from_sql(raw).ok()?;
    let mut dimensions: Vec<ArrayDimension> = Vec::new();
    let mut iter = array.dimensions();
    while let Ok(Some(dimension)) = iter.next() {
        dimensions.push(dimension);
    }
    let mut values = Vec::new();
    let mut iter = array.values();
    while let Ok(Some(value)) = iter.next() {
        values.push(decode(element, value, zone));
    }
    if dimensions.is_empty() {
        return Some(Value::Array(Vec::new()));
    }
    let lengths: Vec<usize> = dimensions.iter().map(|dimension| usize::try_from(dimension.len).unwrap_or(0)).collect();
    Some(nest(&lengths, &mut values.into_iter()))
}

fn nest(lengths: &[usize], values: &mut impl Iterator<Item = Value>) -> Value {
    match lengths.split_first() {
        Some((&length, [])) => Value::Array(values.take(length).collect()),
        Some((&length, rest)) => Value::Array((0..length).map(|_| nest(rest, values)).collect()),
        None => Value::Array(Vec::new()),
    }
}

pub(super) fn numeric(raw: &[u8]) -> Option<String> {
    let word = |at: usize| raw.get(at..at + 2).map(|pair| i16::from_be_bytes([pair[0], pair[1]]));
    let count = usize::try_from(word(0)?).ok()?;
    let weight = i32::from(word(2)?);
    let sign = word(4)? as u16;
    let scale = usize::try_from(word(6)?).ok()?;
    let digits: Vec<i16> = (0..count).map(|index| word(8 + index * 2)).collect::<Option<_>>()?;
    match sign {
        0xC000 => return Some("NaN".to_owned()),
        0xD000 => return Some("Infinity".to_owned()),
        0xF000 => return Some("-Infinity".to_owned()),
        _ => {}
    }
    let digit = |index: i32| usize::try_from(index).ok().and_then(|index| digits.get(index)).copied().unwrap_or(0);
    let mut out = String::new();
    if sign == 0x4000 {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        for index in 0..=weight {
            if index == 0 {
                let _ = write!(out, "{}", digit(index));
            } else {
                let _ = write!(out, "{:04}", digit(index));
            }
        }
    }
    if scale > 0 {
        let mut fraction = String::new();
        let mut index = weight + 1;
        while fraction.len() < scale {
            let _ = write!(fraction, "{:04}", digit(index));
            index += 1;
        }
        fraction.truncate(scale);
        out.push('.');
        out.push_str(&fraction);
    }
    Some(out)
}

fn civil(days_since_2000: i64) -> (i64, u32, u32) {
    let z = days_since_2000 + DAYS_2000_FROM_1970 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn ymd(days_since_2000: i64) -> (String, bool) {
    let (year, month, day) = civil(days_since_2000);
    if year <= 0 {
        (format!("{:04}-{month:02}-{day:02}", 1 - year), true)
    } else {
        (format!("{year:04}-{month:02}-{day:02}"), false)
    }
}

pub(super) fn date(days: i32) -> String {
    match days {
        i32::MAX => "infinity".to_owned(),
        i32::MIN => "-infinity".to_owned(),
        days => {
            let (text, bc) = ymd(days.into());
            if bc { format!("{text} BC") } else { text }
        }
    }
}

pub(super) fn time(micros: i64) -> String {
    let seconds = micros.div_euclid(1_000_000);
    let fraction = micros.rem_euclid(1_000_000);
    let mut out = format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
    if fraction != 0 {
        let digits = format!(".{fraction:06}");
        out.push_str(digits.trim_end_matches('0'));
    }
    out
}

fn offset(east_seconds: i32) -> String {
    let sign = if east_seconds < 0 { '-' } else { '+' };
    let total = east_seconds.unsigned_abs();
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    match (minutes, seconds) {
        (0, 0) => format!("{sign}{hours:02}"),
        (_, 0) => format!("{sign}{hours:02}:{minutes:02}"),
        _ => format!("{sign}{hours:02}:{minutes:02}:{seconds:02}"),
    }
}

pub(super) fn timestamp(micros: i64, east_seconds: Option<i32>) -> String {
    match micros {
        i64::MAX => return "infinity".to_owned(),
        i64::MIN => return "-infinity".to_owned(),
        _ => {}
    }
    let shifted = micros + i64::from(east_seconds.unwrap_or(0)) * 1_000_000;
    let (date, bc) = ymd(shifted.div_euclid(USECS_PER_DAY));
    let mut out = format!("{date} {}", time(shifted.rem_euclid(USECS_PER_DAY)));
    if let Some(east) = east_seconds {
        out.push_str(&offset(east));
    }
    if bc {
        out.push_str(" BC");
    }
    out
}

pub(super) fn interval(months: i32, days: i32, micros: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut negative_before = false;
    let mut push = |value: i64, unit: &str, parts: &mut Vec<String>| {
        if value == 0 {
            return;
        }
        let sign = if value > 0 && negative_before { "+" } else { "" };
        negative_before = value < 0;
        let plural = if value == 1 { "" } else { "s" };
        parts.push(format!("{sign}{value} {unit}{plural}"));
    };
    push(i64::from(months / 12), "year", &mut parts);
    push(i64::from(months % 12), "mon", &mut parts);
    push(i64::from(days), "day", &mut parts);
    if micros != 0 || parts.is_empty() {
        let sign = if micros < 0 {
            "-"
        } else if negative_before {
            "+"
        } else {
            ""
        };
        parts.push(format!("{sign}{}", time(micros.abs())));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_bytes(count: i16, weight: i16, sign: u16, scale: u16, digits: &[i16]) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [count, weight, sign as i16, scale as i16].into_iter().chain(digits.iter().copied()) {
            out.extend_from_slice(&word.to_be_bytes());
        }
        out
    }

    #[test]
    fn numerics_keep_every_digit_and_their_scale() {
        assert_eq!(numeric(&numeric_bytes(2, 0, 0, 2, &[12, 3400])).unwrap(), "12.34");
        assert_eq!(numeric(&numeric_bytes(3, 1, 0x4000, 4, &[1, 2345, 6789])).unwrap(), "-12345.6789");
        assert_eq!(numeric(&numeric_bytes(1, -1, 0, 5, &[50])).unwrap(), "0.00500");
        assert_eq!(numeric(&numeric_bytes(1, 2, 0, 0, &[7])).unwrap(), "700000000");
        assert_eq!(numeric(&numeric_bytes(0, 0, 0, 3, &[])).unwrap(), "0.000");
        assert_eq!(numeric(&numeric_bytes(0, 0, 0xC000, 0, &[])).unwrap(), "NaN");
    }

    #[test]
    fn dates_and_times_read_as_the_server_writes_them() {
        assert_eq!(date(0), "2000-01-01");
        assert_eq!(date(-1), "1999-12-31");
        assert_eq!(date(9190), "2025-02-28");
        assert_eq!(date(-730_120), "0001-12-31 BC");
        assert_eq!(date(-730_119), "0001-01-01");
        assert_eq!(date(i32::MAX), "infinity");
        assert_eq!(time(3_723_500_000), "01:02:03.5");
        assert_eq!(timestamp(0, None), "2000-01-01 00:00:00");
        assert_eq!(timestamp(1_000_000, Some(0)), "2000-01-01 00:00:01+00");
        assert_eq!(offset(-19_800), "-05:30");
        assert_eq!(interval(14, 3, 14_706_000_000), "1 year 2 mons 3 days 04:05:06");
        assert_eq!(interval(0, -1, 7_200_000_000), "-1 days +02:00:00");
        assert_eq!(interval(0, 0, 0), "00:00:00");
    }

    #[test]
    fn timestamps_with_a_zone_are_written_in_the_sessions_zone() {
        let micros = 757_382_400_000_000;
        assert_eq!(timestamp(micros, Some(Zone::named("UTC").offset_at(micros))), "2024-01-01 00:00:00+00");
        let makassar = Zone::named("Asia/Makassar");
        assert_eq!(timestamp(micros, Some(makassar.offset_at(micros))), "2024-01-01 08:00:00+08");
        let india = Zone::named("Asia/Kolkata");
        assert_eq!(timestamp(micros, Some(india.offset_at(micros))), "2024-01-01 05:30:00+05:30");
        let york = Zone::named("America/New_York");
        assert_eq!(timestamp(micros, Some(york.offset_at(micros))), "2023-12-31 19:00:00-05");
        let posix = Zone::named("<+03>-03");
        assert_eq!(timestamp(micros, Some(posix.offset_at(micros))), "2024-01-01 03:00:00+03");
        assert_eq!(Zone::named("Not/AZone").offset_at(micros), 0);
    }
}
