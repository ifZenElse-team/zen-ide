use serde_json::{Value as Json, json};
use zen_db_driver_kit::{Failure, value};

use crate::json::{self, Raw};
use crate::values;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) type_name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Printed {
    pub(crate) columns: Vec<Column>,
    pub(crate) rows: Vec<Vec<Json>>,
    pub(crate) truncated: bool,
    pub(crate) has_result: bool,
    pub(crate) affected: Option<u64>,
    pub(crate) notes: Vec<String>,
    pub(crate) failure: Option<Failure>,
}

fn result_start(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find('{') {
        let at = from + found;
        if text[at + 1..].trim_start().starts_with("\"results\"") {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

fn code_in(line: &str) -> Option<(usize, String)> {
    for prefix in ["ORA-", "PLS-", "SP2-", "TNS-"] {
        let mut from = 0;
        while let Some(found) = line[from..].find(prefix) {
            let at = from + found;
            let digits: String = line[at + 4..].chars().take_while(char::is_ascii_digit).collect();
            if digits.len() >= 4 {
                return Some((at, format!("{prefix}{digits}")));
            }
            from = at + 4;
        }
    }
    None
}

fn numbers_after(line: &str, first: &str, second: &str) -> Option<(usize, usize)> {
    let lower = line.to_ascii_lowercase();
    let take = |from: usize| -> Option<usize> {
        let rest = lower.get(from..)?.trim_start_matches([' ', ':']);
        rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
    };
    let one = lower.find(first)? + first.len();
    let two = lower[one..].find(second)? + one + second.len();
    Some((take(one)?, take(two)?))
}

fn compile_line(line: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim_start();
    let (place, rest) = trimmed.split_once(char::is_whitespace)?;
    let (row, column) = place.split_once('/')?;
    code_in(rest)?;
    Some((row.parse().ok()?, column.parse().ok()?))
}

pub(crate) fn position(statement: &str, row: usize, column: usize) -> Option<usize> {
    let lines: Vec<&str> = statement.split('\n').collect();
    let line = lines.get(row.checked_sub(1)?)?;
    let before: usize = lines[..row - 1].iter().map(|line| line.chars().count() + 1).sum();
    Some(before + column.clamp(1, line.chars().count().max(1)))
}

fn kind_of(code: Option<&str>, message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    match code {
        Some("ORA-01013") => "cancelled",
        Some("ORA-01017" | "ORA-01005" | "ORA-28000" | "ORA-28001" | "ORA-01045") => "authentication",
        Some("ORA-28759" | "ORA-28860" | "ORA-29024" | "ORA-29106" | "ORA-29019") => "tls",
        _ if lower.contains("ssl")
            || lower.contains("certif")
            || lower.contains("pkix")
            || lower.contains("handshake") =>
        {
            "tls"
        }
        Some(code) if code.starts_with("ORA-12") || code.starts_with("TNS-") => "connection",
        Some(
            "ORA-03113" | "ORA-03114" | "ORA-03135" | "ORA-01033" | "ORA-01034" | "ORA-01089" | "ORA-01090"
            | "ORA-02396" | "SP2-0640",
        ) => "connection",
        _ if lower.contains("io error") || lower.contains("not connected") || lower.contains("connection failed") => {
            "connection"
        }
        _ => "server",
    }
}

fn clean(line: &str) -> String {
    let line = line.trim();
    let line = line.strip_prefix("SQL Error:").unwrap_or(line);
    let line = line.trim_start().strip_prefix("Error Message =").unwrap_or(line);
    line.trim().to_owned()
}

pub(crate) fn failure(lines: &[String], statement: &str) -> Option<Failure> {
    let reported = lines.iter().any(|line| {
        let line = line.trim();
        code_in(line).is_some()
            || line.starts_with("Error starting at line")
            || line.starts_with("Error report")
            || line.starts_with("IO Error")
            || line.starts_with("Errors: check compiler log")
            || line.starts_with("Connection failed")
            || line.contains("Error Message =")
    });
    if !reported {
        return None;
    }
    let echoed = lines.iter().position(|line| line.trim_start().starts_with("Error starting at line"));
    let place = lines
        .iter()
        .find_map(|line| numbers_after(line, "error at command line", "column"))
        .or_else(|| {
            lines.iter().find_map(|line| numbers_after(line, "line", ", column").filter(|_| line.contains("ORA-06550")))
        })
        .or_else(|| lines.iter().find_map(|line| compile_line(line)));
    let mut messages: Vec<String> = Vec::new();
    let mut code = None;
    for line in lines {
        if line.trim_start().starts_with("ORA-06550") && lines.iter().filter(|line| code_in(line).is_some()).count() > 1
        {
            continue;
        }
        if let Some((at, found)) = code_in(line) {
            if line.trim_start().chars().next().is_some_and(char::is_numeric) && compile_line(line).is_some() {
                messages.push(line[at..].trim().to_owned());
            } else {
                messages.push(clean(line));
            }
            code.get_or_insert(found);
        } else if line.trim_start().starts_with("IO Error") || line.contains("Error Message =") {
            messages.push(clean(line));
        }
        if messages.len() >= 3 {
            break;
        }
    }
    if messages.is_empty() {
        let report = lines.iter().position(|line| line.trim().starts_with("Error report")).or(echoed);
        messages.extend(
            lines
                .iter()
                .skip(report.map_or(0, |at| at + 1))
                .map(|line| line.trim())
                .filter(|line| !line.is_empty() && !line.starts_with("Error at Command Line"))
                .take(1)
                .map(str::to_owned),
        );
    }
    messages.dedup();
    let message = if messages.is_empty() { "SQLcl reported an error".to_owned() } else { messages.join(" ") };
    let kind = kind_of(code.as_deref(), &message);
    let mut data = json!({ "kind": kind });
    if let Some(code) = &code {
        data["code"] = json!(code);
    }
    if let Some(at) = place.and_then(|(row, column)| position(statement, row, column)) {
        data["position"] = json!(at);
    }
    Some(Failure { code: zen_db_driver_kit::DATABASE_ERROR, message, data: Some(data) })
}

fn affected(line: &str) -> Option<u64> {
    let mut words = line.split_whitespace();
    let count: u64 = words.next()?.parse().ok()?;
    let unit = words.next()?;
    let verb = words.next()?.trim_end_matches('.');
    (matches!(unit, "row" | "rows") && matches!(verb, "inserted" | "updated" | "deleted" | "merged")).then_some(count)
}

fn selected(line: &str) -> bool {
    let line = line.trim().trim_end_matches('.');
    line == "no rows selected" || line.ends_with(" rows selected") || line.ends_with(" row selected")
}

fn columns_of(result: &Raw) -> Vec<Column> {
    result
        .get("columns")
        .map(Raw::items)
        .unwrap_or_default()
        .iter()
        .filter_map(|column| {
            Some(Column {
                name: column.get("name")?.text()?.to_owned(),
                type_name: column.get("type").and_then(Raw::text).unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

fn row_of(item: &Raw, columns: &[Column]) -> Vec<Json> {
    let Raw::Object(fields) = item else {
        return columns.iter().map(|_| value::null()).collect();
    };
    let positional = fields.len() == columns.len();
    columns
        .iter()
        .enumerate()
        .map(|(at, column)| {
            let found = if positional {
                fields.get(at).map(|(_, value)| value)
            } else {
                fields.iter().find(|(name, _)| name.eq_ignore_ascii_case(&column.name)).map(|(_, value)| value)
            };
            found.map_or_else(value::null, |raw| values::cell(raw, &column.type_name))
        })
        .collect()
}

pub(crate) fn read(lines: &[String], statement: &str, max_rows: usize) -> Printed {
    let text = lines.join("\n");
    let mut printed = Printed::default();
    let mut rest = text.clone();
    if let Some(start) = result_start(&text)
        && let Some((document, end)) = json::read(&text, start)
    {
        rest = format!("{}\n{}", &text[..start], &text[end..]);
        if let Some(result) = document.get("results").map(Raw::items).and_then(<[Raw]>::first) {
            printed.has_result = true;
            printed.columns = columns_of(result);
            let items = result.get("items").map(Raw::items).unwrap_or_default();
            printed.truncated = items.len() > max_rows;
            printed.rows = items.iter().take(max_rows).map(|item| row_of(item, &printed.columns)).collect();
        }
    }
    let others: Vec<String> = rest.split('\n').map(str::to_owned).collect();
    printed.failure = failure(&others, statement);
    if printed.failure.is_none() {
        for line in &others {
            let trimmed = line.trim();
            if trimmed.is_empty() || selected(trimmed) {
                continue;
            }
            match affected(trimmed) {
                Some(count) => printed.affected = Some(printed.affected.unwrap_or(0) + count),
                None => printed.notes.push(trimmed.to_owned()),
            }
        }
    }
    printed
}

#[cfg(test)]
mod tests;
