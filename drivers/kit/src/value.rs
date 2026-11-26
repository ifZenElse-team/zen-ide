use serde_json::{Value as Json, json};

pub fn null() -> Json {
    json!({ "kind": "null" })
}

pub fn boolean(value: bool) -> Json {
    json!({ "kind": "bool", "value": value })
}

pub fn integer(value: i64) -> Json {
    json!({ "kind": "integer", "value": value })
}

pub fn float(value: f64) -> Json {
    if value.is_finite() { json!({ "kind": "float", "value": value }) } else { other(&value.to_string(), "float") }
}

pub fn decimal(text: impl Into<String>) -> Json {
    json!({ "kind": "decimal", "value": text.into() })
}

pub fn text(value: impl Into<String>) -> Json {
    json!({ "kind": "text", "value": value.into() })
}

pub fn bytes(value: &[u8]) -> Json {
    json!({ "kind": "bytes", "value": hex(value) })
}

pub fn temporal(text: impl Into<String>, type_name: &str) -> Json {
    json!({ "kind": "temporal", "value": { "text": text.into(), "type": type_name } })
}

pub fn uuid(text: impl Into<String>) -> Json {
    json!({ "kind": "uuid", "value": text.into() })
}

pub fn json_text(text: impl Into<String>) -> Json {
    json!({ "kind": "json", "value": text.into() })
}

pub fn other(text: &str, type_name: &str) -> Json {
    json!({ "kind": "other", "value": { "text": text, "type": type_name } })
}

pub fn column(name: &str, type_name: &str) -> Json {
    json!({ "name": name, "type": type_name })
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * shifted_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 }).unwrap_or(1);
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

pub fn date_text(days: i64) -> String {
    let (year, month, day) = civil(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn time_text(nanos: u64, digits: u8) -> String {
    let seconds = nanos / 1_000_000_000;
    let mut text = format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
    if digits > 0 {
        let fraction = format!("{:09}", nanos % 1_000_000_000);
        text.push('.');
        text.push_str(&fraction[..usize::from(digits.min(9))]);
    }
    text
}

pub fn offset_text(minutes: i64) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let minutes = minutes.abs();
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers_are_civil_dates() {
        assert_eq!(date_text(0), "1970-01-01");
        assert_eq!(date_text(-1), "1969-12-31");
        assert_eq!(date_text(19_723), "2024-01-01");
        assert_eq!(date_text(11_016), "2000-02-29");
        assert_eq!(date_text(-719_162), "0001-01-01");
        assert_eq!(date_text(2_932_896), "9999-12-31");
    }

    #[test]
    fn times_keep_the_digits_asked_for() {
        assert_eq!(time_text(0, 0), "00:00:00");
        assert_eq!(time_text(45_296_123_456_789, 3), "12:34:56.123");
        assert_eq!(time_text(45_296_123_456_789, 7), "12:34:56.1234567");
        assert_eq!(offset_text(-330), "-05:30");
        assert_eq!(offset_text(60), "+01:00");
    }

    #[test]
    fn bytes_go_to_hexadecimal_and_back() {
        assert_eq!(hex(&[0, 0xff, 0x1a]), "00ff1a");
        assert_eq!(unhex("00FF1a"), Some(vec![0, 0xff, 0x1a]));
        assert_eq!(unhex("0x01"), Some(vec![1]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn floats_json_cannot_hold_keep_their_text() {
        assert_eq!(float(1.5), json!({ "kind": "float", "value": 1.5 }));
        assert_eq!(float(f64::NAN)["kind"], "other");
    }
}
