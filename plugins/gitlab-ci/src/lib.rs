use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Keyword = 0,
    String = 1,
    Number = 2,
    Comment = 3,
    Function = 7,
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

const TOP_LEVEL_KEYWORDS: &[&str] = &[
    "stages",
    "variables",
    "include",
    "workflow",
    "default",
    "image",
    "services",
    "cache",
    "before_script",
    "after_script",
    "spec",
];

const VARIABLE_MAPS: &[&str] = &["variables", "matrix", "inputs", "secrets", "id_tokens"];

const JOB_REFERENCES: &[&str] = &["extends", "needs", "dependencies"];

const OPERATORS: &[&str] = &["==", "!=", "=~", "!~", "&&", "||"];

pub fn highlight(text: &str) -> Vec<Span> {
    let mut highlighter =
        Highlighter { text, bytes: text.as_bytes(), spans: Vec::new(), keys: Vec::new(), block: None, flow: None };
    let mut start = 0;
    loop {
        let end = text[start..].find('\n').map_or(text.len(), |newline| start + newline);
        highlighter.line(start, end);
        if end == text.len() {
            break;
        }
        start = end + 1;
    }
    highlighter.spans
}

#[derive(Clone, Copy, Default)]
struct Context {
    jobs: bool,
    expression: bool,
    variables: bool,
}

#[derive(Clone, Copy)]
struct Flow {
    depth: usize,
    context: Context,
    reference: bool,
    items: usize,
}

struct Highlighter<'a> {
    text: &'a str,
    bytes: &'a [u8],
    spans: Vec<Span>,
    keys: Vec<(usize, &'a str)>,
    block: Option<usize>,
    flow: Option<Flow>,
}

impl<'a> Highlighter<'a> {
    fn push(&mut self, start: usize, end: usize, kind: Kind) {
        if end > start {
            self.spans.push(Span { start, end, kind });
        }
    }

    fn line(&mut self, start: usize, end: usize) {
        let line = &self.text[start..end];
        let indent = line.bytes().take_while(|b| *b == b' ').count();
        let blank = line[indent..].trim().is_empty();
        if let Some(parent) = self.block {
            if blank || indent > parent {
                self.plain(start + indent, end);
                return;
            }
            self.block = None;
        }
        if blank {
            return;
        }
        if let Some(flow) = self.flow.take() {
            self.flow(start + indent, end, flow);
            return;
        }
        let first = start + indent;
        if self.bytes[first] == b'#' {
            self.push(first, end, Kind::Comment);
            return;
        }
        if indent == 0
            && (line.starts_with("---") || line.starts_with("..."))
            && line.as_bytes().get(3).is_none_or(|b| b.is_ascii_whitespace())
        {
            self.push(start, start + 3, Kind::Punctuation);
            self.keys.clear();
            self.value(start + 3, end, Context::default(), indent);
            return;
        }
        if indent == 0 && line.starts_with('%') {
            return;
        }
        let mut at = first;
        let mut dash = None;
        while self.bytes[at] == b'-' && (at + 1 == end || matches!(self.bytes[at + 1], b' ' | b'\t')) {
            dash.get_or_insert(at - start);
            at = self.skip_blanks(at + 1, end);
            if at == end {
                break;
            }
        }
        if let Some(column) = dash {
            self.keys.retain(|(key_column, _)| *key_column <= column);
        }
        if at == end {
            return;
        }
        if let Some((key_start, key_end, colon)) = self.key_at(at, end) {
            let column = at - start;
            self.keys.retain(|(key_column, _)| *key_column < column);
            let parent = self.keys.last().map(|(_, key)| *key);
            let key = unquote(&self.text[key_start..key_end]);
            let kind = if key == "<<" {
                Kind::Attribute
            } else {
                match parent {
                    None if TOP_LEVEL_KEYWORDS.contains(&key) => Kind::Keyword,
                    None => Kind::Function,
                    Some(parent) if VARIABLE_MAPS.contains(&parent) => Kind::Constant,
                    Some(_) => Kind::Keyword,
                }
            };
            self.push(key_start, key_end, kind);
            let context = Context {
                jobs: JOB_REFERENCES.contains(&key) || (key == "job" && parent == Some("needs")),
                expression: key == "if",
                variables: VARIABLE_MAPS.contains(&key),
            };
            self.keys.push((column, key));
            self.value(colon + 1, end, context, indent);
        } else {
            let parent = self.keys.last().map(|(_, key)| *key);
            let jobs = dash.is_some() && parent.is_some_and(|parent| JOB_REFERENCES.contains(&parent));
            self.value(at, end, Context { jobs, ..Context::default() }, indent);
        }
    }

    fn key_at(&self, at: usize, end: usize) -> Option<(usize, usize, usize)> {
        let bytes = self.bytes;
        let is_colon =
            |index: usize| bytes[index] == b':' && (index + 1 == end || matches!(bytes[index + 1], b' ' | b'\t'));
        let first = bytes[at];
        if first == b'"' || first == b'\'' {
            let closed = quoted_end(bytes, at, end);
            let colon = self.skip_blanks(closed, end);
            return (colon < end && is_colon(colon)).then_some((at, closed, colon));
        }
        if !self.text[at..end].starts_with("<<") && b"?:,[]{}#&*!|>%@`".contains(&first) {
            return None;
        }
        for index in at..end {
            if bytes[index] == b'#' && index > at && matches!(bytes[index - 1], b' ' | b'\t') {
                return None;
            }
            if is_colon(index) {
                let key_end = at + self.text[at..index].trim_end().len();
                return (key_end > at).then_some((at, key_end, index));
            }
        }
        None
    }

    fn skip_blanks(&self, mut at: usize, end: usize) -> usize {
        while at < end && matches!(self.bytes[at], b' ' | b'\t') {
            at += 1;
        }
        at
    }

    fn comment_start(&self, at: usize, end: usize) -> usize {
        (at..end)
            .find(|&index| self.bytes[index] == b'#' && (index == at || matches!(self.bytes[index - 1], b' ' | b'\t')))
            .unwrap_or(end)
    }

    fn value(&mut self, at: usize, end: usize, context: Context, indent: usize) {
        let bytes = self.bytes;
        let mut at = self.skip_blanks(at, end);
        let mut reference = false;
        while at < end {
            match bytes[at] {
                b'#' => {
                    self.push(at, end, Kind::Comment);
                    return;
                }
                b'&' | b'*' | b'!' => {
                    let length = self.text[at..end]
                        .bytes()
                        .take_while(|b| !b.is_ascii_whitespace() && !b",[]{}".contains(b))
                        .count();
                    reference |= &self.text[at..at + length] == "!reference";
                    self.push(at, at + length, Kind::Attribute);
                    at = self.skip_blanks(at + length, end);
                }
                b'|' | b'>' => {
                    let indicators = 1 + self.text[at + 1..end]
                        .bytes()
                        .take_while(|b| matches!(b, b'+' | b'-') || b.is_ascii_digit())
                        .count();
                    let rest = self.skip_blanks(at + indicators, end);
                    if rest == end || bytes[rest] == b'#' {
                        self.block = Some(indent);
                        self.push(rest, end, Kind::Comment);
                        return;
                    }
                    self.scalar(at, end, context);
                    return;
                }
                b'[' | b'{' => {
                    self.flow(at, end, Flow { depth: 0, context, reference, items: 0 });
                    return;
                }
                b'"' | b'\'' => {
                    let quote = bytes[at];
                    let closed = quoted_end(bytes, at, end);
                    if context.expression && closed > at + 1 && bytes[closed - 1] == quote {
                        self.push(at, at + 1, Kind::String);
                        self.expression(at + 1, closed - 1, Some(quote));
                        self.push(closed - 1, closed, Kind::String);
                    } else if context.jobs {
                        self.push(at, closed, Kind::Function);
                    } else {
                        self.variables(at, closed, Some(Kind::String));
                    }
                    at = self.skip_blanks(closed, end);
                    if at < end && bytes[at] == b'#' {
                        self.push(at, end, Kind::Comment);
                        return;
                    }
                    self.plain(at, end);
                    return;
                }
                _ => {
                    self.scalar(at, end, context);
                    return;
                }
            }
        }
    }

    fn scalar(&mut self, at: usize, end: usize, context: Context) {
        let comment = self.comment_start(at, end);
        let scalar_end = at + self.text[at..comment].trim_end().len();
        let scalar = &self.text[at..scalar_end];
        if context.expression {
            self.expression(at, scalar_end, None);
        } else if is_constant(scalar) {
            self.push(at, scalar_end, Kind::Constant);
        } else if is_number(scalar) {
            self.push(at, scalar_end, Kind::Number);
        } else if context.jobs {
            self.push(at, scalar_end, Kind::Function);
        } else {
            self.plain(at, scalar_end);
        }
        self.push(comment, end, Kind::Comment);
    }

    fn flow(&mut self, mut at: usize, end: usize, mut flow: Flow) {
        let bytes = self.bytes;
        let line_start = at;
        while at < end {
            match bytes[at] {
                b' ' | b'\t' | b',' => at += 1,
                b'[' | b'{' => {
                    flow.depth += 1;
                    at += 1;
                }
                b']' | b'}' => {
                    flow.depth = flow.depth.saturating_sub(1);
                    at += 1;
                    if flow.depth == 0 {
                        let rest = self.comment_start(at, end);
                        self.push(rest, end, Kind::Comment);
                        return;
                    }
                }
                b'#' if at == line_start || matches!(bytes[at - 1], b' ' | b'\t') => {
                    self.push(at, end, Kind::Comment);
                    break;
                }
                b'&' | b'*' | b'!' => {
                    let length = self.text[at..end]
                        .bytes()
                        .take_while(|b| !b.is_ascii_whitespace() && !b",[]{}".contains(b))
                        .count();
                    self.push(at, at + length, Kind::Attribute);
                    at += length;
                }
                b'"' | b'\'' => {
                    let closed = quoted_end(bytes, at, end);
                    let colon = self.skip_blanks(closed, end);
                    if colon < end && bytes[colon] == b':' {
                        self.push(at, closed, if flow.context.variables { Kind::Constant } else { Kind::Keyword });
                        at = colon + 1;
                    } else {
                        if flow.context.jobs || (flow.reference && flow.items == 0) {
                            self.push(at, closed, Kind::Function);
                        } else {
                            self.variables(at, closed, Some(Kind::String));
                        }
                        flow.items += 1;
                        at = closed;
                    }
                }
                _ => {
                    let mut scalar_end = at;
                    while scalar_end < end {
                        let b = bytes[scalar_end];
                        if b",[]{}".contains(&b)
                            || (b == b'#' && scalar_end > at && matches!(bytes[scalar_end - 1], b' ' | b'\t'))
                            || (b == b':' && (scalar_end + 1 == end || b" \t,]}".contains(&bytes[scalar_end + 1])))
                        {
                            break;
                        }
                        scalar_end += 1;
                    }
                    let trimmed = at + self.text[at..scalar_end].trim_end().len();
                    let scalar = &self.text[at..trimmed];
                    if scalar_end < end && bytes[scalar_end] == b':' {
                        self.push(at, trimmed, if flow.context.variables { Kind::Constant } else { Kind::Keyword });
                        at = scalar_end + 1;
                        continue;
                    }
                    if is_constant(scalar) {
                        self.push(at, trimmed, Kind::Constant);
                    } else if is_number(scalar) {
                        self.push(at, trimmed, Kind::Number);
                    } else if flow.context.jobs || (flow.reference && flow.items == 0) {
                        self.push(at, trimmed, Kind::Function);
                    } else if flow.reference {
                        self.push(at, trimmed, Kind::Keyword);
                    } else {
                        self.plain(at, trimmed);
                    }
                    flow.items += 1;
                    at = if scalar_end > at {
                        scalar_end
                    } else {
                        at + self.text[at..].chars().next().map_or(1, char::len_utf8)
                    };
                }
            }
        }
        if flow.depth > 0 {
            self.flow = Some(flow);
        }
    }

    fn expression(&mut self, mut at: usize, end: usize, quote: Option<u8>) {
        let bytes = self.bytes;
        while at < end {
            let rest = &self.text[at..end];
            let b = bytes[at];
            if b == b'$' {
                let length = variable_length(rest).unwrap_or(0);
                self.push(at, at + length, Kind::Constant);
                at += length.max(1);
            } else if OPERATORS.iter().any(|operator| rest.starts_with(operator)) {
                self.push(at, at + 2, Kind::Punctuation);
                at += 2;
            } else if (b == b'"' || b == b'\'') && Some(b) != quote {
                let closed = quoted_end(bytes, at, end);
                self.variables(at, closed, Some(Kind::String));
                at = closed;
            } else if b == b'/' {
                let mut index = at + 1;
                while index < end {
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'/' => {
                            index += 1;
                            while index < end && bytes[index].is_ascii_alphabetic() {
                                index += 1;
                            }
                            break;
                        }
                        _ => index += 1,
                    }
                }
                let index = index.min(end);
                self.push(at, index, Kind::String);
                at = index;
            } else if b.is_ascii_alphabetic() || b == b'_' {
                let length = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
                if &rest[..length] == "null" {
                    self.push(at, at + length, Kind::Constant);
                }
                at += length;
            } else {
                at += rest.chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    fn plain(&mut self, at: usize, end: usize) {
        let bytes = self.bytes;
        let mut segment = at;
        let mut index = at;
        while index < end {
            if matches!(bytes[index], b'"' | b'\'') {
                let closed = quoted_end(bytes, index, end);
                if closed > index + 1 && bytes[closed - 1] == bytes[index] {
                    self.variables(segment, index, None);
                    self.variables(index, closed, Some(Kind::String));
                    index = closed;
                    segment = closed;
                    continue;
                }
            }
            index += 1;
        }
        self.variables(segment, end, None);
    }

    fn variables(&mut self, at: usize, end: usize, base: Option<Kind>) {
        let bytes = self.bytes;
        let mut segment = at;
        let mut index = at;
        while index < end {
            if bytes[index] == b'$' {
                if index + 1 < end && bytes[index + 1] == b'$' {
                    index += 2;
                    continue;
                }
                if let Some(length) = variable_length(&self.text[index..end]) {
                    if let Some(kind) = base {
                        self.push(segment, index, kind);
                    }
                    self.push(index, index + length, Kind::Constant);
                    index += length;
                    segment = index;
                    continue;
                }
            }
            index += 1;
        }
        if let Some(kind) = base {
            self.push(segment, end, kind);
        }
    }
}

fn variable_length(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let is_name = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_';
    match bytes.get(1)? {
        b'{' => {
            let name = bytes[2..].iter().take_while(|b| is_name(b)).count();
            (name > 0 && bytes.get(2 + name) == Some(&b'}')).then_some(3 + name)
        }
        b if b.is_ascii_alphabetic() || *b == b'_' => Some(1 + bytes[1..].iter().take_while(|b| is_name(b)).count()),
        _ => None,
    }
}

fn quoted_end(bytes: &[u8], at: usize, end: usize) -> usize {
    let quote = bytes[at];
    let mut index = at + 1;
    while index < end {
        match bytes[index] {
            b'\\' if quote == b'"' => index += 2,
            b if b == quote => {
                if quote == b'\'' && index + 1 < end && bytes[index + 1] == b'\'' {
                    index += 2;
                } else {
                    return index + 1;
                }
            }
            _ => index += 1,
        }
    }
    end
}

fn unquote(key: &str) -> &str {
    let quoted = key.len() >= 2
        && ((key.starts_with('"') && key.ends_with('"')) || (key.starts_with('\'') && key.ends_with('\'')));
    if quoted { &key[1..key.len() - 1] } else { key }
}

fn is_constant(scalar: &str) -> bool {
    matches!(scalar, "true" | "True" | "TRUE" | "false" | "False" | "FALSE" | "null" | "Null" | "NULL" | "~")
}

fn is_number(scalar: &str) -> bool {
    let unsigned = scalar.strip_prefix(['-', '+']).unwrap_or(scalar);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(scalar, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    if let Some(hex) = unsigned.strip_prefix("0x") {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if let Some(octal) = unsigned.strip_prefix("0o") {
        return !octal.is_empty() && octal.bytes().all(|b| (b'0'..=b'7').contains(&b));
    }
    let bytes = unsigned.as_bytes();
    let mut at = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut digits = at;
    if bytes.get(at) == Some(&b'.') {
        let fraction = bytes[at + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        digits += fraction;
        at += 1 + fraction;
    }
    if digits == 0 {
        return false;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(at + 1), Some(b'+' | b'-')));
        let exponent = bytes[(at + 1 + sign).min(bytes.len())..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exponent == 0 {
            return false;
        }
        at += 1 + sign + exponent;
    }
    at == bytes.len()
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
    let encoded = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let text = if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" };
        encode(&highlight(text))
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = encoded;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

const RESOURCES: &[(&str, &[u8])] = &[("ci.json", include_bytes!("../schemas/ci.json"))];

pub fn resource(path: &str) -> Option<&'static [u8]> {
    RESOURCES.iter().find(|(name, _)| *name == path).map(|(_, bytes)| *bytes)
}

#[allow(unsafe_code, reason = "a WebAssembly export needs an unmangled name (ADR-0031, rule 3)")]
#[cfg_attr(target_family = "wasm", unsafe(no_mangle))]
pub extern "C" fn zen_resource(ptr: i32, len: i32) -> i64 {
    let found = INPUT.with_borrow(|input| {
        let valid = input.as_ptr() as usize as i32 == ptr && usize::try_from(len).is_ok_and(|len| len <= input.len());
        let path = if valid { std::str::from_utf8(&input[..len as usize]).unwrap_or("") } else { "" };
        resource(path).unwrap_or_default().to_vec()
    });
    OUTPUT.with_borrow_mut(|output| {
        *output = found;
        ((output.as_ptr() as usize as i64) << 32) | output.len() as i64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        highlight(text).into_iter().map(|span| (&text[span.start..span.end], span.kind)).collect()
    }

    fn assert_well_formed(text: &str) {
        let mut previous = 0;
        for span in highlight(text) {
            assert!(
                span.start >= previous
                    && span.end > span.start
                    && span.end <= text.len()
                    && text.is_char_boundary(span.start)
                    && text.is_char_boundary(span.end),
                "{text:?}: {span:?}"
            );
            previous = span.end;
        }
    }

    const PIPELINE: &str = r#"# Build, test and deploy.
include:
  - template: Security/SAST.gitlab-ci.yml
  - local: '/ci/common.yml'

stages: [build, test, deploy]

variables:
  GIT_DEPTH: 10
  IMAGE_TAG: "$CI_REGISTRY_IMAGE:${CI_COMMIT_SHORT_SHA}"

workflow:
  rules:
    - if: '$CI_PIPELINE_SOURCE == "merge_request_event"'
    - if: $CI_COMMIT_BRANCH =~ /^release\/.*$/i && $CI_COMMIT_TAG == null

.defaults: &defaults
  image: rust:1.99
  interruptible: true
  retry: 2

build:
  <<: *defaults
  stage: build
  script:
    - cargo build --release # optimised
    - echo "Built $CI_COMMIT_REF_NAME"
  artifacts:
    paths:
      - target/release/
    expire_in: 1 week

test:
  extends: .defaults
  needs: [build]
  before_script:
    - !reference [.setup, script]
  script: |
    cargo test
    # not a comment: part of the script
    echo $CI_JOB_ID
  rules:
    - if: $CI_MERGE_REQUEST_IID
      changes:
        - src/**/*
      when: always
  parallel:
    matrix:
      - TARGET: [x86_64, aarch64]
"#;

    #[test]
    fn highlights_a_pipeline() {
        let found = kinds(PIPELINE);
        for expected in [
            ("# Build, test and deploy.", Kind::Comment),
            ("include", Kind::Keyword),
            ("template", Kind::Keyword),
            ("local", Kind::Keyword),
            ("'/ci/common.yml'", Kind::String),
            ("stages", Kind::Keyword),
            ("variables", Kind::Keyword),
            ("GIT_DEPTH", Kind::Constant),
            ("10", Kind::Number),
            ("IMAGE_TAG", Kind::Constant),
            ("\"", Kind::String),
            ("$CI_REGISTRY_IMAGE", Kind::Constant),
            (":", Kind::String),
            ("${CI_COMMIT_SHORT_SHA}", Kind::Constant),
            ("workflow", Kind::Keyword),
            ("rules", Kind::Keyword),
            ("if", Kind::Keyword),
            ("'", Kind::String),
            ("$CI_PIPELINE_SOURCE", Kind::Constant),
            ("==", Kind::Punctuation),
            ("\"merge_request_event\"", Kind::String),
            ("=~", Kind::Punctuation),
            ("/^release\\/.*$/i", Kind::String),
            ("&&", Kind::Punctuation),
            ("null", Kind::Constant),
            (".defaults", Kind::Function),
            ("&defaults", Kind::Attribute),
            ("image", Kind::Keyword),
            ("true", Kind::Constant),
            ("2", Kind::Number),
            ("build", Kind::Function),
            ("<<", Kind::Attribute),
            ("*defaults", Kind::Attribute),
            ("stage", Kind::Keyword),
            ("script", Kind::Keyword),
            ("# optimised", Kind::Comment),
            ("\"Built ", Kind::String),
            ("$CI_COMMIT_REF_NAME", Kind::Constant),
            ("artifacts", Kind::Keyword),
            ("paths", Kind::Keyword),
            ("expire_in", Kind::Keyword),
            ("test", Kind::Function),
            ("extends", Kind::Keyword),
            ("needs", Kind::Keyword),
            ("!reference", Kind::Attribute),
            (".setup", Kind::Function),
            ("$CI_JOB_ID", Kind::Constant),
            ("$CI_MERGE_REQUEST_IID", Kind::Constant),
            ("changes", Kind::Keyword),
            ("when", Kind::Keyword),
            ("parallel", Kind::Keyword),
            ("matrix", Kind::Keyword),
            ("TARGET", Kind::Constant),
        ] {
            assert!(found.contains(&expected), "{expected:?} in {found:?}");
        }
        let functions: Vec<&str> =
            found.iter().filter(|(_, kind)| *kind == Kind::Function).map(|(text, _)| *text).collect();
        assert_eq!(functions, [".defaults", "build", "test", ".defaults", "build", ".setup"]);
        assert!(found.contains(&("script", Kind::Keyword)));
        assert!(!found.iter().any(|(text, _)| text.contains("not a comment")));
        for plain in ["cargo build --release", "target/release/", "always", "1 week", "x86_64"] {
            assert!(!found.iter().any(|(text, _)| *text == plain), "{plain}");
        }
        assert_well_formed(PIPELINE);
    }

    #[test]
    fn reads_keys_by_context() {
        let text = "pages:\n  script: [make]\n  variables: {MODE: fast}\n  secrets:\n    DB_PASSWORD:\n      vault: db/pw\nspec:\n  inputs:\n    stage:\n      default: test\n---\n\"quoted job\":\n  needs:\n    - job: lint\n    - other\n  dependencies: [\"a\"]\n";
        let found = kinds(text);
        for expected in [
            ("pages", Kind::Function),
            ("MODE", Kind::Constant),
            ("DB_PASSWORD", Kind::Constant),
            ("vault", Kind::Keyword),
            ("spec", Kind::Keyword),
            ("inputs", Kind::Keyword),
            ("stage", Kind::Constant),
            ("default", Kind::Keyword),
            ("---", Kind::Punctuation),
            ("\"quoted job\"", Kind::Function),
            ("job", Kind::Keyword),
            ("lint", Kind::Function),
            ("other", Kind::Function),
            ("\"a\"", Kind::Function),
        ] {
            assert!(found.contains(&expected), "{expected:?} in {found:?}");
        }
        assert_well_formed(text);
    }

    #[test]
    fn escaped_dollars_and_urls_stay_plain() {
        let found = kinds("job:\n  script:\n    - echo $$HOME http://x:8080 ${A} $1 don't\n");
        assert_eq!(found, [("job", Kind::Function), ("script", Kind::Keyword), ("${A}", Kind::Constant)]);
        let found = kinds("- echo 'it''s' \"$HOME/x\" it's\n");
        assert_eq!(
            found,
            [("'it''s'", Kind::String), ("\"", Kind::String), ("$HOME", Kind::Constant), ("/x\"", Kind::String)]
        );
    }

    #[test]
    fn multi_line_flow_collections() {
        let found = kinds("job:\n  needs: [\n    build, # first\n    test]\n  stage: x\n");
        assert_eq!(
            found,
            [
                ("job", Kind::Function),
                ("needs", Kind::Keyword),
                ("build", Kind::Function),
                ("# first", Kind::Comment),
                ("test", Kind::Function),
                ("stage", Kind::Keyword),
            ]
        );
    }

    #[test]
    fn survives_any_input() {
        let pieces = [
            "- ",
            ": ",
            ":",
            "|",
            ">",
            "\n",
            "\n  ",
            "#",
            " #",
            "$",
            "${",
            "}",
            "$$",
            "'",
            "\"",
            "[",
            "]",
            "{",
            ",",
            "!reference",
            "&a",
            "*a",
            "<<",
            "if",
            "variables",
            "/",
            "\\",
            "==",
            "---",
            "1",
            "x",
            "é",
            "日",
        ];
        for a in pieces {
            for b in pieces {
                for c in pieces {
                    assert_well_formed(&format!("{a}{b}{c}"));
                    assert_well_formed(&format!("j:\n  {a}{b}{c}"));
                    assert_well_formed(&format!("- if: '{a}{b}{c}"));
                }
            }
        }
    }

    #[test]
    fn encodes_records() {
        assert_eq!(encode(&[Span { start: 1, end: 3, kind: Kind::Function }]), [1, 0, 0, 0, 3, 0, 0, 0, 7, 0, 0, 0]);
    }

    #[test]
    fn every_shipped_schema_is_a_json_object() {
        let text = std::str::from_utf8(resource("ci.json").unwrap()).unwrap();
        assert!(text.trim_start().starts_with('{') && text.trim_end().ends_with('}'));
        assert!(resource("../Cargo.toml").is_none() && resource("").is_none());
    }
}
