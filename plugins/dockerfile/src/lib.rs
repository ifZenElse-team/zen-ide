#[allow(unreachable_pub, reason = "the shared YAML highlighter is public in its own crate")]
#[path = "../../yaml/src/yaml.rs"]
mod yaml;

pub mod assist;

pub mod compose;

pub mod engine;

use std::cell::RefCell;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    Attribute = 9,
    Constant = 10,
    Punctuation = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

const INSTRUCTIONS: &[&str] = &[
    "FROM",
    "RUN",
    "CMD",
    "LABEL",
    "MAINTAINER",
    "EXPOSE",
    "ENV",
    "ADD",
    "COPY",
    "ENTRYPOINT",
    "VOLUME",
    "USER",
    "WORKDIR",
    "ARG",
    "ONBUILD",
    "STOPSIGNAL",
    "HEALTHCHECK",
    "SHELL",
];

const JSON_FORMS: &[&str] = &["RUN", "CMD", "ENTRYPOINT", "SHELL", "VOLUME", "ADD", "COPY"];

const HEREDOC_FORMS: &[&str] = &["RUN", "COPY", "ADD"];

const SHELL_KEYWORDS: &[&str] =
    &["if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function"];

const SHELL_LEADS: &[&str] = &["if", "then", "else", "elif", "while", "until", "do"];

const DIRECTIVES: &[&str] = &["syntax", "escape", "check"];

pub fn highlight(text: &str) -> Vec<Span> {
    if is_dockerfile(text) {
        return highlight_dockerfile(text);
    }
    yaml::highlight(text)
        .into_iter()
        .map(|span| {
            let kind = match span.kind {
                yaml::Kind::Keyword => Kind::Keyword,
                yaml::Kind::String => Kind::String,
                yaml::Kind::Number => Kind::Number,
                yaml::Kind::Comment => Kind::Comment,
                yaml::Kind::Attribute => Kind::Attribute,
                yaml::Kind::Constant => Kind::Constant,
                yaml::Kind::Punctuation => Kind::Punctuation,
            };
            Span { start: span.start, end: span.end, kind }
        })
        .collect()
}

pub fn is_dockerfile(text: &str) -> bool {
    let Some(line) = text
        .lines()
        .map(|line| line.trim_start_matches([' ', '\t']))
        .find(|line| !line.trim_end().is_empty() && !line.starts_with('#'))
    else {
        return true;
    };
    let length = line.bytes().take_while(u8::is_ascii_alphabetic).count();
    let separated = matches!(line.as_bytes().get(length), None | Some(b' ' | b'\t' | b'\r'));
    separated && INSTRUCTIONS.iter().any(|name| name.eq_ignore_ascii_case(&line[..length]))
}

fn highlight_dockerfile(text: &str) -> Vec<Span> {
    let mut highlighter = Highlighter { text, bytes: text.as_bytes(), spans: Vec::new(), escape: b'\\' };
    let mut directives = true;
    let mut continued: Option<Instruction> = None;
    let mut heredocs: VecDeque<Heredoc> = VecDeque::new();
    let mut start = 0;
    while start < text.len() {
        let newline = text[start..].find('\n').map_or(text.len(), |offset| start + offset);
        let end = if newline > start && highlighter.bytes[newline - 1] == b'\r' { newline - 1 } else { newline };
        highlighter.line(start, end, &mut directives, &mut continued, &mut heredocs);
        start = newline + 1;
    }
    highlighter.spans
}

struct Heredoc {
    name: String,
    strip_tabs: bool,
}

#[derive(Clone, Copy)]
struct Instruction {
    name: Option<&'static str>,
    positional: usize,
    json: bool,
    command: bool,
}

struct Highlighter<'a> {
    text: &'a str,
    bytes: &'a [u8],
    spans: Vec<Span>,
    escape: u8,
}

impl Highlighter<'_> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            self.spans.push(Span { start, end, kind });
        }
    }

    fn line(
        &mut self,
        start: usize,
        end: usize,
        directives: &mut bool,
        continued: &mut Option<Instruction>,
        heredocs: &mut VecDeque<Heredoc>,
    ) {
        if continued.is_none()
            && let Some(heredoc) = heredocs.front()
        {
            let line = &self.text[start..end];
            let delimiter = if heredoc.strip_tabs { line.trim_start_matches('\t') } else { line };
            if delimiter == heredoc.name {
                heredocs.pop_front();
            }
            self.push(start, end, Kind::String);
            return;
        }
        let content = start + self.text[start..end].bytes().take_while(|b| matches!(b, b' ' | b'\t')).count();
        if *directives {
            if let Some(escape) = directive(&self.text[content..end]) {
                self.push(content, end, Kind::Attribute);
                if let Some(escape) = escape {
                    self.escape = escape;
                }
                return;
            }
            *directives = false;
        }
        if content == end {
            return;
        }
        if self.bytes[content] == b'#' {
            self.push(content, end, Kind::Comment);
            return;
        }
        let trimmed = end - self.text[content..end].bytes().rev().take_while(|b| matches!(b, b' ' | b'\t')).count();
        let continues = self.bytes[trimmed - 1] == self.escape;
        let arguments_end = if continues { trimmed - 1 } else { end };
        let (mut instruction, at) = match continued.take() {
            Some(instruction) => (instruction, content),
            None => self.instruction(content, arguments_end),
        };
        self.arguments(&mut instruction, at, arguments_end, heredocs);
        if continues {
            self.push(trimmed - 1, trimmed, Kind::Punctuation);
            *continued = Some(instruction);
        }
    }

    fn instruction(&mut self, at: usize, to: usize) -> (Instruction, usize) {
        let mut instruction = Instruction { name: None, positional: 0, json: false, command: false };
        let mut at = at;
        loop {
            let length = self.bytes[at..to].iter().take_while(|b| b.is_ascii_alphabetic()).count();
            let word = &self.text[at..at + length];
            let separated = at + length == to || matches!(self.bytes[at + length], b' ' | b'\t');
            let Some(name) = INSTRUCTIONS.iter().find(|name| name.eq_ignore_ascii_case(word)).filter(|_| separated)
            else {
                break;
            };
            self.push(at, at + length, Kind::Keyword);
            instruction.name = Some(name);
            instruction.command = *name == "RUN";
            at += length;
            if *name != "ONBUILD" {
                break;
            }
            at += self.bytes[at..to].iter().take_while(|b| matches!(b, b' ' | b'\t')).count();
        }
        (instruction, at)
    }

    fn arguments(&mut self, instruction: &mut Instruction, mut at: usize, to: usize, heredocs: &mut VecDeque<Heredoc>) {
        while at < to {
            let c = self.bytes[at];
            if matches!(c, b' ' | b'\t') {
                at += 1;
                continue;
            }
            let name = instruction.name.unwrap_or("");
            let shell = name == "RUN" && !instruction.json;
            let word_start = at == 0 || matches!(self.bytes[at - 1], b' ' | b'\t' | b'\n');
            if c == b'[' && instruction.positional == 0 && !instruction.json && JSON_FORMS.contains(&name) {
                instruction.json = true;
                self.push(at, at + 1, Kind::Punctuation);
                at += 1;
                continue;
            }
            if instruction.json && matches!(c, b'[' | b']' | b',') {
                self.push(at, at + 1, Kind::Punctuation);
                at += 1;
                continue;
            }
            if c == b'"' || c == b'\'' {
                at = self.string(at, to);
                positional(instruction, word_start);
                instruction.command = false;
                continue;
            }
            if c == b'$'
                && let Some(end) = self.variable(at, to)
            {
                self.push(at, end, Kind::Constant);
                at = end;
                positional(instruction, word_start);
                instruction.command = false;
                continue;
            }
            if c == b'-'
                && self.bytes.get(at + 1) == Some(&b'-')
                && word_start
                && instruction.positional == 0
                && !instruction.json
            {
                let end = at + self.bytes[at..to].iter().take_while(|b| !matches!(b, b' ' | b'\t')).count();
                self.push(at, end, Kind::Attribute);
                at = end;
                continue;
            }
            if c == b'<'
                && !instruction.json
                && HEREDOC_FORMS.contains(&name)
                && let Some((end, heredoc)) = self.heredoc(at, to)
            {
                self.push(at, end, Kind::String);
                heredocs.push_back(heredoc);
                at = end;
                positional(instruction, word_start);
                instruction.command = false;
                continue;
            }
            if shell {
                let pair = self.bytes.get(at + 1).copied();
                if (c == b'&' && pair == Some(b'&')) || (c == b'|' && pair == Some(b'|')) {
                    self.push(at, at + 2, Kind::Keyword);
                    instruction.command = true;
                    at += 2;
                    continue;
                }
                if matches!(c, b';' | b'|' | b'&' | b'(' | b')' | b'{' | b'}') {
                    instruction.command = c != b')' && c != b'}';
                    at += 1;
                    continue;
                }
            }
            let stops = |b: &u8| {
                matches!(b, b' ' | b'\t' | b'"' | b'\'' | b'$')
                    || (instruction.json && matches!(b, b'[' | b']' | b','))
                    || (shell && matches!(b, b';' | b'|' | b'&' | b'(' | b')'))
            };
            let mut end = at + self.bytes[at..to].iter().take_while(|b| !stops(b)).count();
            if end == at {
                end = at + self.text[at..].chars().next().map_or(1, char::len_utf8);
            }
            let word = &self.text[at..end];
            let whole = end == to || matches!(self.bytes[end], b' ' | b'\t' | b';' | b'&' | b'|');
            match name {
                "EXPOSE" if whole && is_port(word) => self.push(at, end, Kind::Number),
                "STOPSIGNAL" if whole && word.bytes().all(|b| b.is_ascii_digit()) => {
                    self.push(at, end, Kind::Number);
                }
                "FROM" if whole && word_start && instruction.positional == 1 && word.eq_ignore_ascii_case("AS") => {
                    self.push(at, end, Kind::Keyword);
                }
                "HEALTHCHECK"
                    if word_start
                        && instruction.positional == 0
                        && (word.eq_ignore_ascii_case("CMD") || word.eq_ignore_ascii_case("NONE")) =>
                {
                    self.push(at, end, Kind::Keyword);
                    instruction.name = Some("CMD");
                    at = end;
                    continue;
                }
                "RUN" if shell && word_start && whole && instruction.command && SHELL_KEYWORDS.contains(&word) => {
                    self.push(at, end, Kind::Keyword);
                    instruction.command = SHELL_LEADS.contains(&word);
                    positional(instruction, word_start);
                    at = end;
                    continue;
                }
                _ => {}
            }
            positional(instruction, word_start);
            instruction.command = false;
            at = end;
        }
    }

    fn string(&mut self, at: usize, to: usize) -> usize {
        let quote = self.bytes[at];
        let mut piece = at;
        let mut index = at + 1;
        loop {
            if index >= to {
                self.push(piece, to, Kind::String);
                return to;
            }
            match self.bytes[index] {
                b if b == quote => {
                    self.push(piece, index + 1, Kind::String);
                    return index + 1;
                }
                b'\\' if quote == b'"' => index += 2,
                b'$' if quote == b'"' => match self.variable(index, to) {
                    Some(end) => {
                        self.push(piece, index, Kind::String);
                        self.push(index, end, Kind::Constant);
                        piece = end;
                        index = end;
                    }
                    None => index += 1,
                },
                _ => index += 1,
            }
        }
    }

    fn variable(&self, at: usize, to: usize) -> Option<usize> {
        if at + 1 >= to {
            return None;
        }
        if self.bytes[at + 1] == b'{' {
            return Some(self.text[at + 2..to].find('}').map_or(to, |close| at + 2 + close + 1));
        }
        let length = self.bytes[at + 1..to].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
        (length > 0).then_some(at + 1 + length)
    }

    fn heredoc(&self, at: usize, to: usize) -> Option<(usize, Heredoc)> {
        let bytes = &self.bytes[..to];
        if bytes.get(at + 1) != Some(&b'<') || bytes.get(at + 2) == Some(&b'<') {
            return None;
        }
        let mut index = at + 2;
        let strip_tabs = bytes.get(index) == Some(&b'-');
        index += usize::from(strip_tabs);
        let quote = bytes.get(index).copied().filter(|b| matches!(b, b'"' | b'\''));
        index += usize::from(quote.is_some());
        let length = bytes[index.min(to)..].iter().take_while(|b| b.is_ascii_alphanumeric() || **b == b'_').count();
        if length == 0 {
            return None;
        }
        let name = self.text[index..index + length].to_owned();
        index += length;
        if let Some(quote) = quote {
            if bytes.get(index) != Some(&quote) {
                return None;
            }
            index += 1;
        }
        Some((index, Heredoc { name, strip_tabs }))
    }
}

fn positional(instruction: &mut Instruction, word_start: bool) {
    if word_start || instruction.positional == 0 {
        instruction.positional += 1;
    }
}

fn directive(line: &str) -> Option<Option<u8>> {
    let rest = line.strip_prefix('#')?.trim_start_matches([' ', '\t']);
    let length = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let key = &rest[..length];
    let value = rest[length..].trim_start_matches([' ', '\t']).strip_prefix('=')?.trim();
    if !DIRECTIVES.iter().any(|known| known.eq_ignore_ascii_case(key)) {
        return None;
    }
    Some(if key.eq_ignore_ascii_case("escape") {
        match value {
            "`" => Some(b'`'),
            "\\" => Some(b'\\'),
            _ => None,
        }
    } else {
        None
    })
}

fn is_port(word: &str) -> bool {
    let (ports, protocol) = word.split_once('/').unwrap_or((word, "tcp"));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let (low, high) = ports.split_once('-').unwrap_or((ports, "0"));
    digits(low) && digits(high) && !protocol.is_empty() && protocol.bytes().all(|b| b.is_ascii_alphabetic())
}

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn encode(spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::with_capacity(spans.len() * 12);
    for span in spans {
        for value in [span.start as u32, span.end as u32, span.kind as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_abi() -> i32 {
    1
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_alloc(len: i32) -> i32 {
    INPUT.with_borrow_mut(|input| {
        input.clear();
        input.resize(usize::try_from(len).unwrap_or(0), 0);
        input.as_mut_ptr() as usize as i32
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_highlight(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| encode(&highlight(std::str::from_utf8(input).unwrap_or(""))))
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_complete(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| {
        let Some((caret, text)) = input.split_first_chunk::<4>() else { return Vec::new() };
        let text = std::str::from_utf8(text).unwrap_or("");
        let caret = u32::from_le_bytes(*caret) as usize;
        let found = if is_dockerfile(text) { assist::complete(text, caret) } else { compose::complete(text, caret) };
        found
            .filter(|found| !found.items.is_empty())
            .map_or_else(Vec::new, |found| completion_json(&found).into_bytes())
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_problems(ptr: i32, len: i32) -> i64 {
    call(ptr, len, |input| {
        let text = std::str::from_utf8(input).unwrap_or("");
        let found = if is_dockerfile(text) { assist::problems(text) } else { compose::problems(text) };
        problems_json(&found).into_bytes()
    })
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_run_targets(ptr: i32, len: i32) -> i64 {
    call(ptr, len, run_targets_request)
}

pub fn run_targets_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return Vec::new() };
    let text = std::str::from_utf8(&input[split + 1..]).unwrap_or("");
    if is_dockerfile(text) {
        return Vec::new();
    }
    run_targets_json(&compose::run_targets(text)).into_bytes()
}

pub fn run_targets_json(targets: &[compose::Target]) -> String {
    let mut out = String::from("[");
    for (index, target) in targets.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!("{{\"line\":{},\"name\":", target.line));
        json_string(&mut out, &target.name);
        out.push_str(",\"id\":");
        json_string(&mut out, &target.id);
        out.push_str(",\"program\":\"docker\",\"cwd\":\"${fileDir}\",\"kind\":\"main\",\"args\":[");
        for (at, arg) in target.args.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            json_string(&mut out, arg);
        }
        out.push_str("]}");
    }
    out.push(']');
    out
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_tool_rows(ptr: i32, len: i32) -> i64 {
    call(ptr, len, tool_rows_request)
}

pub fn tool_rows_request(input: &[u8]) -> Vec<u8> {
    let Some(split) = input.iter().position(|byte| *byte == 0) else { return Vec::new() };
    let section = std::str::from_utf8(&input[..split]).unwrap_or("");
    let output = String::from_utf8_lossy(&input[split + 1..]);
    tool_rows_json(&engine::rows(section, &output)).into_bytes()
}

pub fn tool_rows_json(rows: &[engine::Row]) -> String {
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        json_string(&mut out, &row.id);
        out.push_str(",\"columns\":[");
        for (at, column) in row.columns.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            json_string(&mut out, column);
        }
        out.push(']');
        if let Some(state) = &row.state {
            out.push_str(",\"state\":");
            json_string(&mut out, state);
        }
        out.push('}');
    }
    out.push(']');
    out
}

fn call(ptr: i32, len: i32, run: impl FnOnce(&[u8]) -> Vec<u8>) -> i64 {
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        run(if valid { &input[..len as usize] } else { &[] })
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn completion_json(found: &assist::Completion) -> String {
    let mut out = format!("{{\"from\":{},\"items\":[", found.from);
    for (index, item) in found.items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        json_string(&mut out, &item.label);
        out.push_str(",\"kind\":");
        json_string(&mut out, item.kind);
        out.push_str(",\"insert\":");
        json_string(&mut out, &item.insert);
        if let Some(detail) = item.detail {
            out.push_str(",\"detail\":");
            json_string(&mut out, detail);
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub fn problems_json(problems: &[assist::Problem]) -> String {
    let mut out = String::from("[");
    for (index, problem) in problems.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let severity = match problem.severity {
            assist::Severity::Error => "error",
            assist::Severity::Warning => "warning",
        };
        out.push_str(&format!(
            "{{\"start\":{},\"end\":{},\"severity\":\"{severity}\",\"message\":",
            problem.start, problem.end
        ));
        json_string(&mut out, &problem.message);
        out.push('}');
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_rows_are_valid_json_for_the_host() {
        let input = b"containers\0{\"ID\":\"c1\",\"Names\":\"say \\\"hi\\\"\",\"State\":\"paused\"}\n";
        let json: serde_json::Value = serde_json::from_slice(&tool_rows_request(input)).unwrap();
        assert_eq!(json, serde_json::json!([{ "id": "c1", "columns": ["say \"hi\"", "", "", ""], "state": "paused" }]));
        assert_eq!(tool_rows_request(b"no section"), b"");
    }

    #[test]
    fn the_declared_tool_window_uses_only_the_declared_program_and_whole_row_arguments() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
        assert_eq!(manifest["runs"], serde_json::json!(["docker"]));
        let sections = manifest["toolWindows"][0]["sections"].as_array().unwrap();
        let ids: Vec<&str> = sections.iter().filter_map(|section| section["id"].as_str()).collect();
        assert_eq!(ids, ["containers", "images", "volumes", "networks"]);
        for section in sections {
            let columns: Vec<&str> = section["columns"].as_array().unwrap().iter().filter_map(|c| c.as_str()).collect();
            let commands = std::iter::once(&section["list"]).chain(section["actions"].as_array().unwrap());
            for command in commands {
                assert_eq!(command["program"], "docker");
                for arg in command["args"].as_array().unwrap().iter().filter_map(|arg| arg.as_str()) {
                    if let Some(name) = arg.strip_prefix("${row.").and_then(|rest| rest.strip_suffix('}')) {
                        assert!(name == "id" || columns.contains(&name), "{arg}");
                    } else {
                        assert!(!arg.contains("${"), "{arg}");
                    }
                }
            }
            for title in std::iter::once(&section["title"]).chain(section["columnTitles"].as_object().unwrap().values())
            {
                assert!(title["en"].is_string() && title["ru"].is_string(), "{title}");
            }
            for action in section["actions"].as_array().unwrap() {
                let destroys = ["remove", "prune"].contains(&action["id"].as_str().unwrap());
                assert_eq!(action["kind"] == "destroy", destroys, "{action}");
                assert!(action["title"]["en"].is_string() && action["title"]["ru"].is_string());
            }
        }
    }

    #[test]
    fn run_targets_are_valid_json_for_the_host() {
        let text = "services:\n  web-1:\n    image: x\n";
        let json: serde_json::Value = serde_json::from_str(&run_targets_json(&compose::run_targets(text))).unwrap();
        assert_eq!(json[1]["name"], "web-1");
        assert_eq!(json[1]["program"], "docker");
        assert_eq!(json[1]["args"][4], "web-1");
        assert_eq!(json[0]["cwd"], "${fileDir}");
    }

    #[test]
    fn completion_and_problems_are_valid_json_for_the_host() {
        let text = "FROM alpine\nCOPPY \"a\\b\" c\nCO";
        let problems: serde_json::Value = serde_json::from_str(&problems_json(&assist::problems(text))).unwrap();
        assert_eq!(problems[0]["severity"], "error");
        assert_eq!(problems[0]["message"], "Unknown instruction: COPPY");
        let found = assist::complete(text, text.len()).unwrap();
        let completion: serde_json::Value = serde_json::from_str(&completion_json(&found)).unwrap();
        assert_eq!(completion["items"][0]["insert"], "COPY ");
        assert_eq!(completion["from"], text.len() - 2);
    }

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    #[test]
    fn highlights_a_multi_stage_build() {
        let text = "# syntax=docker/dockerfile:1\n\
                    # Build stage\n\
                    FROM --platform=$BUILDPLATFORM golang:1.23 AS builder\n\
                    ARG VERSION=${VERSION:-dev}\n\
                    RUN --mount=type=cache,target=/root/.cache go build -o /app . && \\\n    \
                    echo \"built $VERSION\"\n\
                    FROM scratch\n\
                    COPY --from=builder --chown=1000:1000 /app /app\n\
                    EXPOSE 8080/tcp 53/udp\n\
                    ENTRYPOINT [\"/app\", \"--port\"]\n";
        assert_eq!(
            kinds(text),
            [
                ("# syntax=docker/dockerfile:1", Kind::Attribute),
                ("# Build stage", Kind::Comment),
                ("FROM", Kind::Keyword),
                ("--platform=$BUILDPLATFORM", Kind::Attribute),
                ("AS", Kind::Keyword),
                ("ARG", Kind::Keyword),
                ("${VERSION:-dev}", Kind::Constant),
                ("RUN", Kind::Keyword),
                ("--mount=type=cache,target=/root/.cache", Kind::Attribute),
                ("&&", Kind::Keyword),
                ("\\", Kind::Punctuation),
                ("\"built ", Kind::String),
                ("$VERSION", Kind::Constant),
                ("\"", Kind::String),
                ("FROM", Kind::Keyword),
                ("COPY", Kind::Keyword),
                ("--from=builder", Kind::Attribute),
                ("--chown=1000:1000", Kind::Attribute),
                ("EXPOSE", Kind::Keyword),
                ("8080/tcp", Kind::Number),
                ("53/udp", Kind::Number),
                ("ENTRYPOINT", Kind::Keyword),
                ("[", Kind::Punctuation),
                ("\"/app\"", Kind::String),
                (",", Kind::Punctuation),
                ("\"--port\"", Kind::String),
                ("]", Kind::Punctuation),
            ]
        );
    }

    #[test]
    fn highlights_shell_keywords_at_command_positions_only() {
        let text = "RUN if [ -f x ]; then echo done || true; fi && for f in a b; do ls --all $f; done";
        let keywords: Vec<&str> =
            kinds(text).into_iter().filter(|(_, kind)| *kind == Kind::Keyword).map(|(word, _)| word).collect();
        assert_eq!(keywords, ["RUN", "if", "then", "||", "fi", "&&", "for", "do", "done"]);
        assert!(!kinds(text).iter().any(|(_, kind)| *kind == Kind::Attribute), "--all is an argument, not a flag");
        assert!(kinds("CMD if then").iter().all(|(word, _)| *word == "CMD"), "only RUN is read as shell");
    }

    #[test]
    fn highlights_heredocs() {
        let text = "RUN <<EOF\napt-get update\n  EOF\nEOF\nCOPY <<-\"END\" /etc/conf\n\tkey=$v\n\tEND\nUSER app";
        assert_eq!(
            kinds(text),
            [
                ("RUN", Kind::Keyword),
                ("<<EOF", Kind::String),
                ("apt-get update", Kind::String),
                ("  EOF", Kind::String),
                ("EOF", Kind::String),
                ("COPY", Kind::Keyword),
                ("<<-\"END\"", Kind::String),
                ("\tkey=$v", Kind::String),
                ("\tEND", Kind::String),
                ("USER", Kind::Keyword),
            ]
        );
    }

    #[test]
    fn reads_instructions_case_insensitively() {
        let text = "from alpine as base\nonbuild run make\nHealthCheck --interval=30s CMD [\"curl\", \"-f\"]\n\
                    STOPSIGNAL 9\nRUNNER x\nLABEL a=\"b\" c='$d'";
        assert_eq!(
            kinds(text),
            [
                ("from", Kind::Keyword),
                ("as", Kind::Keyword),
                ("onbuild", Kind::Keyword),
                ("run", Kind::Keyword),
                ("HealthCheck", Kind::Keyword),
                ("--interval=30s", Kind::Attribute),
                ("CMD", Kind::Keyword),
                ("[", Kind::Punctuation),
                ("\"curl\"", Kind::String),
                (",", Kind::Punctuation),
                ("\"-f\"", Kind::String),
                ("]", Kind::Punctuation),
                ("STOPSIGNAL", Kind::Keyword),
                ("9", Kind::Number),
                ("LABEL", Kind::Keyword),
                ("\"b\"", Kind::String),
                ("'$d'", Kind::String),
            ]
        );
    }

    #[test]
    fn follows_continuations_and_the_escape_directive() {
        let text = "RUN echo a \\\n# note\n\n    && echo b\nENV X=1";
        let found = kinds(text);
        assert!(found.contains(&("# note", Kind::Comment)) && found.contains(&("&&", Kind::Keyword)), "{found:?}");
        let text = "# escape=`\nRUN dir C:\\ `\n  && echo\n# syntax=x\n";
        assert_eq!(
            kinds(text),
            [
                ("# escape=`", Kind::Attribute),
                ("RUN", Kind::Keyword),
                ("`", Kind::Punctuation),
                ("&&", Kind::Keyword),
                ("# syntax=x", Kind::Comment),
            ]
        );
    }

    #[test]
    fn tells_dockerfiles_from_compose_files() {
        for dockerfile in [
            "FROM alpine",
            "from alpine\n",
            "# syntax=docker/dockerfile:1\n# escape=`\n\nARG BASE=alpine\nFROM $BASE",
            "\n  # A comment\n\tRun echo",
            "\r\nFROM\r\n",
            "MAINTAINER me",
            "",
            "# only a comment\n",
        ] {
            assert!(is_dockerfile(dockerfile), "{dockerfile:?}");
        }
        for compose in [
            "services:\n  web:\n    image: nginx\n",
            "version: \"3.9\"\nservices: {}\n",
            "# Development stack\nservices:\n",
            "---\nservices:\n",
            "x-common: &common\n  restart: always\n",
            "from: x\n",
            "RUNNER x",
            "name: app",
        ] {
            assert!(!is_dockerfile(compose), "{compose:?}");
        }
    }

    #[test]
    fn highlights_a_compose_file() {
        let text = "\
# syntax=not-a-directive-here
services:
  web:
    build: {context: ., dockerfile: Dockerfile}
    image: \"app:${TAG:-latest}\"
    ports:
      - 8080:80
    restart: no
    command: >
      run --port 80
x-logging: &logging
  max-size: 10
";
        assert_eq!(
            kinds(text),
            [
                ("# syntax=not-a-directive-here", Kind::Comment),
                ("services", Kind::Keyword),
                (":", Kind::Punctuation),
                ("web", Kind::Keyword),
                (":", Kind::Punctuation),
                ("build", Kind::Keyword),
                (":", Kind::Punctuation),
                ("{", Kind::Punctuation),
                ("context", Kind::Keyword),
                (":", Kind::Punctuation),
                (",", Kind::Punctuation),
                ("dockerfile", Kind::Keyword),
                (":", Kind::Punctuation),
                ("}", Kind::Punctuation),
                ("image", Kind::Keyword),
                (":", Kind::Punctuation),
                ("\"app:", Kind::String),
                ("${TAG:-latest}", Kind::Constant),
                ("\"", Kind::String),
                ("ports", Kind::Keyword),
                (":", Kind::Punctuation),
                ("-", Kind::Punctuation),
                ("restart", Kind::Keyword),
                (":", Kind::Punctuation),
                ("no", Kind::Constant),
                ("command", Kind::Keyword),
                (":", Kind::Punctuation),
                (">", Kind::Punctuation),
                ("run --port 80", Kind::String),
                ("x-logging", Kind::Keyword),
                (":", Kind::Punctuation),
                ("&logging", Kind::Attribute),
                ("max-size", Kind::Keyword),
                (":", Kind::Punctuation),
                ("10", Kind::Number),
            ]
        );
        assert_eq!(highlight("FROM x")[0].kind, Kind::Keyword);
        assert_eq!(kinds("# base\nversion: '3'")[1], ("version", Kind::Keyword));
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "'", "\"", "$", "${", "}", "#", "\n", "\\", "`", "<<", "-", "--", "[", ",", "RUN ", "FROM ", "EXPOSE ",
            "1", "&&", "if", " ", "\r", "é", "日", ": ", "- ", "|", "&", "{",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    let text = format!("{a}{b}{c}");
                    let mut previous = 0;
                    for span in highlight(&text) {
                        assert!(
                            span.start >= previous
                                && span.end > span.start
                                && span.end <= text.len()
                                && text.is_char_boundary(span.start)
                                && text.is_char_boundary(span.end),
                            "{text:?}"
                        );
                        previous = span.end;
                    }
                }
            }
        }
    }

    #[test]
    fn encodes_records() {
        assert_eq!(
            encode(&[Span { start: 1, end: 3, kind: Kind::Punctuation }]),
            [1, 0, 0, 0, 3, 0, 0, 0, 11, 0, 0, 0]
        );
    }
}
