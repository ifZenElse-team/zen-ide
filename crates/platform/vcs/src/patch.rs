use std::cell::OnceCell;
use std::collections::HashMap;
use std::fmt;

use crate::merge::{self, ChunkKind};

pub const MAX_ITEMS: usize = 100_000;

pub const MAX_LINE: usize = 1_000_000_000;

pub const MAX_BYTES: usize = 64 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub mail: Option<Mail>,
    pub files: Vec<FilePatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mail {
    pub commit: Option<String>,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub body: String,
}

impl Mail {
    pub fn message(&self) -> String {
        if self.body.is_empty() { self.subject.clone() } else { format!("{}\n\n{}", self.subject, self.body) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileKind {
    #[default]
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Modified => "modified",
            FileKind::Added => "added",
            FileKind::Deleted => "deleted",
            FileKind::Renamed => "renamed",
            FileKind::Copied => "copied",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binary {
    Data(Vec<Vec<u8>>),
    Marker,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePatch {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub kind: FileKind,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub similarity: Option<String>,
    pub index: Option<String>,
    pub hunks: Vec<Hunk>,
    pub binary: Option<Binary>,
}

impl FilePatch {
    pub fn path(&self) -> &str {
        self.new_path.as_deref().or(self.old_path.as_deref()).unwrap_or_default()
    }

    pub fn old_blob(&self) -> Option<&str> {
        let index = self.index.as_deref()?;
        let old = index.split("..").next()?;
        (!old.is_empty() && old.bytes().any(|byte| byte != b'0') && old.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then_some(old)
    }

    pub fn is_special(&self) -> bool {
        [&self.old_mode, &self.new_mode].iter().any(|mode| matches!(mode.as_deref(), Some("120000" | "160000")))
    }

    pub fn executable_change(&self) -> Option<bool> {
        let executable = |mode: &Option<String>| mode.as_deref() == Some("100755");
        match self.kind {
            FileKind::Deleted => None,
            FileKind::Added => executable(&self.new_mode).then_some(true),
            _ if self.new_mode.is_none() || self.old_mode == self.new_mode => None,
            _ => Some(executable(&self.new_mode)).filter(|now| *now != executable(&self.old_mode)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub section: Vec<u8>,
    pub lines: Vec<Line>,
}

impl Hunk {
    pub fn preimage(&self) -> Vec<&[u8]> {
        self.lines.iter().filter(|line| line.kind != LineKind::Added).map(|line| line.text.as_slice()).collect()
    }

    pub fn postimage(&self) -> Vec<&[u8]> {
        self.lines.iter().filter(|line| line.kind != LineKind::Removed).map(|line| line.text.as_slice()).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Removed,
    Added,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub text: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchError {
    Empty,
    BadHunk(usize),
    Truncated(usize),
    UnsafePath(String),
    Strip(String),
    OutsideBase(String),
    TooLarge,
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Empty => write!(f, "the patch has no changes"),
            PatchError::BadHunk(line) => write!(f, "line {line}: not a hunk header"),
            PatchError::Truncated(line) => write!(f, "line {line}: the hunk ends early"),
            PatchError::UnsafePath(path) => write!(f, "unsafe path '{path}'"),
            PatchError::Strip(path) => write!(f, "'{path}' has too few folders to strip"),
            PatchError::OutsideBase(path) => write!(f, "'{path}' is outside the base folder"),
            PatchError::TooLarge => write!(f, "the patch is too large"),
        }
    }
}

impl std::error::Error for PatchError {}

pub fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|byte| *byte == b'\n').collect()
}

fn trim_end(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &line[..end]
}

fn text(line: &[u8]) -> String {
    String::from_utf8_lossy(trim_end(line)).into_owned()
}

fn starts(line: &[u8], prefix: &str) -> bool {
    line.starts_with(prefix.as_bytes())
}

fn after<'a>(line: &'a [u8], prefix: &str) -> &'a [u8] {
    trim_end(line.get(prefix.len()..).unwrap_or_default())
}

pub fn parse(bytes: &[u8]) -> Result<Patch, PatchError> {
    if bytes.len() > MAX_BYTES {
        return Err(PatchError::TooLarge);
    }
    let lines = split_lines(bytes);
    let (mail, mut at) = parse_mail(&lines);
    let mut files = Vec::new();
    while at < lines.len() {
        let line = lines[at];
        if starts(line, "diff --git ") {
            let (file, next) = parse_git_file(&lines, at)?;
            files.push(file);
            at = next;
        } else if starts(line, "--- ") && lines.get(at + 1).is_some_and(|next| starts(next, "+++ ")) {
            let (file, next) = parse_plain_file(&lines, at)?;
            files.push(file);
            at = next;
        } else {
            at += 1;
        }
        if files.len() > MAX_ITEMS {
            return Err(PatchError::TooLarge);
        }
    }
    if files.is_empty() {
        return Err(PatchError::Empty);
    }
    Ok(Patch { mail, files })
}

fn parse_mail(lines: &[&[u8]]) -> (Option<Mail>, usize) {
    let mut at = 0;
    let mut mail = Mail::default();
    if let Some(first) = lines.first().filter(|line| starts(line, "From ")) {
        let commit = text(first).split_whitespace().nth(1).map(str::to_owned);
        mail.commit = commit.filter(|id| id.len() >= 7 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        at = 1;
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    while let Some(line) = lines.get(at) {
        let value = text(line);
        if value.is_empty() {
            break;
        }
        if value.starts_with([' ', '\t']) {
            if let Some((_, last)) = headers.last_mut() {
                last.push(' ');
                last.push_str(value.trim());
            }
        } else if let Some((name, rest)) = value.split_once(':').filter(|(name, _)| is_header_name(name)) {
            headers.push((name.to_ascii_lowercase(), rest.trim().to_owned()));
        } else {
            break;
        }
        at += 1;
    }
    let header = |name: &str| headers.iter().find(|(key, _)| key == name).map(|(_, value)| decode_words(value));
    let (Some(author), subject) = (header("from"), header("subject")) else {
        return (None, 0);
    };
    if subject.is_none() && mail.commit.is_none() {
        return (None, 0);
    }
    mail.author = author;
    mail.date = header("date").unwrap_or_default();
    mail.subject = strip_patch_prefix(&subject.unwrap_or_default()).to_owned();
    let mut body: Vec<String> = Vec::new();
    while let Some(line) = lines.get(at) {
        if trim_end(line) == b"---" || starts(line, "diff --git ") || starts(line, "Index: ") {
            break;
        }
        if starts(line, "--- ") && lines.get(at + 1).is_some_and(|next| starts(next, "+++ ")) {
            break;
        }
        body.push(text(line));
        at += 1;
    }
    mail.body = body.join("\n").trim().to_owned();
    (Some(mail), at)
}

fn is_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn strip_patch_prefix(subject: &str) -> &str {
    let subject = subject.trim();
    if subject.starts_with('[')
        && let Some(end) = subject.find(']')
        && subject[1..end].to_ascii_uppercase().contains("PATCH")
    {
        return subject[end + 1..].trim_start();
    }
    subject
}

pub fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, candidate) = rest.split_at(start);
        let decoded = decode_word(candidate);
        let Some((word, used)) = decoded else {
            out.push_str(before);
            out.push_str("=?");
            rest = &candidate[2..];
            last_was_word = false;
            continue;
        };
        if !(last_was_word && before.trim().is_empty()) {
            out.push_str(before);
        }
        out.push_str(&word);
        rest = &candidate[used..];
        last_was_word = true;
    }
    out.push_str(rest);
    out
}

fn decode_word(word: &str) -> Option<(String, usize)> {
    let inner = word.get(2..)?;
    let (charset, inner) = inner.split_once('?')?;
    let (encoding, inner) = inner.split_once('?')?;
    let end = inner.find("?=")?;
    let data = &inner[..end];
    let used = 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
    let bytes = match encoding.to_ascii_lowercase().as_str() {
        "q" => {
            let mut bytes = Vec::new();
            let raw = data.as_bytes();
            let mut i = 0;
            while i < raw.len() {
                match raw[i] {
                    b'_' => bytes.push(b' '),
                    b'=' => {
                        let hex = raw.get(i + 1..i + 3).and_then(|hex| std::str::from_utf8(hex).ok());
                        match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                            Some(byte) => {
                                bytes.push(byte);
                                i += 2;
                            }
                            None => bytes.push(b'='),
                        }
                    }
                    byte => bytes.push(byte),
                }
                i += 1;
            }
            bytes
        }
        "b" => base64(data)?,
        _ => return None,
    };
    let label = charset.split('*').next().unwrap_or_default();
    let decoded = match encoding_rs::Encoding::for_label(label.as_bytes()) {
        Some(encoding) => encoding.decode_without_bom_handling(&bytes).0.into_owned(),
        None => String::from_utf8_lossy(&bytes).into_owned(),
    };
    Some((decoded, used))
}

fn base64(data: &str) -> Option<Vec<u8>> {
    let value = |byte: u8| -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in data.bytes().filter(|byte| *byte != b'=' && !byte.is_ascii_whitespace()) {
        buffer = (buffer << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
        buffer &= (1 << bits) - 1;
    }
    Some(out)
}

const EXTENDED: [&str; 12] = [
    "old mode ",
    "new mode ",
    "deleted file mode ",
    "new file mode ",
    "similarity index ",
    "dissimilarity index ",
    "rename from ",
    "rename to ",
    "copy from ",
    "copy to ",
    "index ",
    "Binary files ",
];

fn parse_git_file(lines: &[&[u8]], start: usize) -> Result<(FilePatch, usize), PatchError> {
    let header = after(lines[start], "diff --git ");
    let mut file = FilePatch::default();
    let (mut from, mut to) = (None, None);
    let mut at = start + 1;
    let mut binary = None;
    while let Some(line) = lines.get(at) {
        if starts(line, "GIT binary patch") {
            let (data, next) = binary_lines(lines, at + 1);
            binary = Some(Binary::Data(data));
            at = next;
            break;
        }
        let Some(prefix) = EXTENDED.iter().find(|prefix| starts(line, prefix)) else { break };
        let value = String::from_utf8_lossy(after(line, prefix)).into_owned();
        let mode = || Some(value.clone()).filter(|mode| is_mode(mode));
        match *prefix {
            "old mode " => file.old_mode = mode(),
            "new mode " => file.new_mode = mode(),
            "deleted file mode " => {
                file.kind = FileKind::Deleted;
                file.old_mode = mode();
            }
            "new file mode " => {
                file.kind = FileKind::Added;
                file.new_mode = mode();
            }
            "similarity index " | "dissimilarity index " => file.similarity = Some(value),
            "rename from " | "copy from " => {
                file.kind = if *prefix == "rename from " { FileKind::Renamed } else { FileKind::Copied };
                from = Some(unquote(after(line, prefix)));
            }
            "rename to " | "copy to " => to = Some(unquote(after(line, prefix))),
            "index " => {
                let mut parts = value.splitn(2, ' ');
                file.index = parts.next().map(str::to_owned);
                if let Some(mode) = parts.next().filter(|mode| is_mode(mode)) {
                    file.old_mode.get_or_insert_with(|| mode.to_owned());
                    file.new_mode.get_or_insert_with(|| mode.to_owned());
                }
            }
            _ => binary = Some(Binary::Marker),
        }
        at += 1;
    }
    let (old, new) = split_git_header(header, from.as_deref(), to.as_deref());
    for (path, named) in [(&old, &from), (&new, &to)] {
        if let (Some(path), Some(named)) = (path, named)
            && !path.ends_with(named.as_str())
        {
            return Err(PatchError::UnsafePath(named.clone()));
        }
    }
    file.old_path = old;
    file.new_path = new;
    if binary.is_none()
        && let Some(line) = lines.get(at)
        && starts(line, "--- ")
        && lines.get(at + 1).is_some_and(|next| starts(next, "+++ "))
    {
        let old = header_path(after(line, "--- "));
        let new = header_path(after(lines[at + 1], "+++ "));
        if old.is_some() || file.kind != FileKind::Added {
            file.old_path = old;
        }
        if new.is_some() || file.kind != FileKind::Deleted {
            file.new_path = new;
        }
        at += 2;
    }
    if file.kind == FileKind::Added {
        file.old_path = None;
    }
    if file.kind == FileKind::Deleted {
        file.new_path = None;
    }
    if binary.is_none() {
        let (hunks, next) = parse_hunks(lines, at)?;
        file.hunks = hunks;
        at = next;
    }
    file.binary = binary;
    if file.old_path.is_none() && file.new_path.is_none() {
        return Err(PatchError::UnsafePath(String::from_utf8_lossy(header).into_owned()));
    }
    Ok((file, at))
}

fn is_mode(mode: &str) -> bool {
    mode.len() == 6 && mode.bytes().all(|byte| (b'0'..=b'7').contains(&byte))
}

fn binary_lines(lines: &[&[u8]], start: usize) -> (Vec<Vec<u8>>, usize) {
    let mut data = Vec::new();
    let mut at = start;
    while let Some(line) = lines.get(at) {
        let value = trim_end(line);
        let data_line = value.first().is_some_and(|first| first.is_ascii_alphabetic())
            && value.iter().all(|byte| byte.is_ascii_graphic());
        if !(value.is_empty() || starts(line, "literal ") || starts(line, "delta ") || data_line) {
            break;
        }
        if starts(line, "diff --git ") {
            break;
        }
        data.push(value.to_vec());
        at += 1;
    }
    while data.last().is_some_and(Vec::is_empty) {
        data.pop();
    }
    (data, at)
}

fn parse_plain_file(lines: &[&[u8]], start: usize) -> Result<(FilePatch, usize), PatchError> {
    let old = header_path(after(lines[start], "--- "));
    let new = header_path(after(lines[start + 1], "+++ "));
    let kind = match (&old, &new) {
        (None, Some(_)) => FileKind::Added,
        (Some(_), None) => FileKind::Deleted,
        _ => FileKind::Modified,
    };
    let (hunks, next) = parse_hunks(lines, start + 2)?;
    if old.is_none() && new.is_none() {
        return Err(PatchError::UnsafePath("/dev/null".to_owned()));
    }
    Ok((FilePatch { old_path: old, new_path: new, kind, hunks, ..FilePatch::default() }, next))
}

fn header_path(value: &[u8]) -> Option<String> {
    let path = if value.first() == Some(&b'"') {
        let (path, _) = unquote_prefix(value);
        path
    } else {
        let end = value.iter().position(|byte| *byte == b'\t').unwrap_or(value.len());
        String::from_utf8_lossy(&value[..end]).trim_end().to_owned()
    };
    (path != "/dev/null" && !path.is_empty()).then_some(path)
}

fn unquote(value: &[u8]) -> String {
    if value.first() == Some(&b'"') { unquote_prefix(value).0 } else { String::from_utf8_lossy(value).into_owned() }
}

fn unquote_prefix(value: &[u8]) -> (String, usize) {
    let mut out = Vec::new();
    let mut i = 1;
    while i < value.len() {
        match value[i] {
            b'"' => return (String::from_utf8_lossy(&out).into_owned(), i + 1),
            b'\\' => {
                let Some(&next) = value.get(i + 1) else { break };
                match next {
                    b'0'..=b'7' => {
                        let digits: Vec<u8> = value[i + 1..]
                            .iter()
                            .take(3)
                            .take_while(|byte| (b'0'..=b'7').contains(*byte))
                            .copied()
                            .collect();
                        let number = digits.iter().fold(0u32, |number, digit| number * 8 + u32::from(digit - b'0'));
                        out.push((number & 0xff) as u8);
                        i += 1 + digits.len();
                        continue;
                    }
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'v' => out.push(11),
                    other => out.push(other),
                }
                i += 2;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), value.len())
}

fn split_git_header(header: &[u8], from: Option<&str>, to: Option<&str>) -> (Option<String>, Option<String>) {
    if header.first() == Some(&b'"') {
        let (old, used) = unquote_prefix(header);
        let rest = header.get(used..).unwrap_or_default();
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);
        return (Some(old), Some(unquote(rest)));
    }
    let value = String::from_utf8_lossy(header).into_owned();
    if let Some(quote) = value.find(" \"") {
        let old = value[..quote].to_owned();
        return (Some(old), Some(unquote(&value.as_bytes()[quote + 1..])));
    }
    let spaces: Vec<usize> = value.match_indices(' ').map(|(index, _)| index).collect();
    if let (Some(from), Some(to)) = (from, to) {
        for &space in &spaces {
            let (old, new) = (&value[..space], &value[space + 1..]);
            if old.ends_with(from) && new.ends_with(to) {
                return (Some(old.to_owned()), Some(new.to_owned()));
            }
        }
    }
    let tail = |path: &str| path.split_once('/').map(|(_, rest)| rest.to_owned()).unwrap_or_else(|| path.to_owned());
    for &space in &spaces {
        let (old, new) = (&value[..space], &value[space + 1..]);
        if tail(old) == tail(new) {
            return (Some(old.to_owned()), Some(new.to_owned()));
        }
    }
    match spaces.get(spaces.len() / 2) {
        Some(&space) => (Some(value[..space].to_owned()), Some(value[space + 1..].to_owned())),
        None => (Some(value.clone()), Some(value)),
    }
}

fn parse_hunks(lines: &[&[u8]], start: usize) -> Result<(Vec<Hunk>, usize), PatchError> {
    let mut hunks = Vec::new();
    let mut at = start;
    while let Some(line) = lines.get(at) {
        if !starts(line, "@@ ") {
            break;
        }
        let (mut hunk, header_end) = hunk_header(line).ok_or(PatchError::BadHunk(at + 1))?;
        hunk.section = header_end;
        let begin = at;
        at += 1;
        let (mut old, mut new) = (hunk.old_lines, hunk.new_lines);
        while old > 0 || new > 0 {
            let Some(&line) = lines.get(at) else { return Err(PatchError::Truncated(begin + 1)) };
            let last = at + 1 == lines.len();
            let (kind, body) = match line.first() {
                Some(b' ') => (LineKind::Context, &line[1..]),
                Some(b'-') => (LineKind::Removed, &line[1..]),
                Some(b'+') => (LineKind::Added, &line[1..]),
                Some(b'\\') => {
                    no_newline(&mut hunk);
                    at += 1;
                    continue;
                }
                Some(b'\n') | Some(b'\r') => (LineKind::Context, line),
                _ => return Err(PatchError::Truncated(begin + 1)),
            };
            match kind {
                LineKind::Context if old > 0 && new > 0 => {
                    old -= 1;
                    new -= 1;
                }
                LineKind::Removed if old > 0 => old -= 1,
                LineKind::Added if new > 0 => new -= 1,
                _ => return Err(PatchError::Truncated(begin + 1)),
            }
            let mut text = body.to_vec();
            if last && text.last() != Some(&b'\n') {
                text.push(b'\n');
            }
            hunk.lines.push(Line { kind, text });
            at += 1;
        }
        if lines.get(at).is_some_and(|line| line.first() == Some(&b'\\')) {
            no_newline(&mut hunk);
            at += 1;
        }
        hunks.push(hunk);
        if hunks.len() > MAX_ITEMS {
            return Err(PatchError::TooLarge);
        }
    }
    Ok((hunks, at))
}

fn no_newline(hunk: &mut Hunk) {
    if let Some(line) = hunk.lines.last_mut()
        && line.text.last() == Some(&b'\n')
    {
        line.text.pop();
    }
}

fn hunk_header(line: &[u8]) -> Option<(Hunk, Vec<u8>)> {
    let rest = line.get(3..)?;
    let end = rest.windows(2).position(|pair| pair == b"@@")?;
    let ranges = std::str::from_utf8(&rest[..end]).ok()?.trim();
    let section = trim_end(rest.get(end + 2..).unwrap_or_default()).to_vec();
    let (old, new) = ranges.split_once(' ')?;
    let (old_start, old_lines) = range(old.strip_prefix('-')?)?;
    let (new_start, new_lines) = range(new.trim().strip_prefix('+')?)?;
    Some((Hunk { old_start, old_lines, new_start, new_lines, ..Hunk::default() }, section))
}

fn range(text: &str) -> Option<(usize, usize)> {
    let (start, count) = match text.split_once(',') {
        Some((start, count)) => (start, count),
        None => (text, "1"),
    };
    let start: usize = start.parse().ok()?;
    let count: usize = count.parse().ok()?;
    (start <= MAX_LINE && count <= MAX_LINE).then_some((start, count))
}

pub fn safe_path(path: &str) -> Result<String, PatchError> {
    let unsafe_path = || PatchError::UnsafePath(path.to_owned());
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.bytes().any(|byte| byte < 0x20)
        || path.as_bytes().get(1) == Some(&b':')
    {
        return Err(unsafe_path());
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(unsafe_path()),
            part if is_git_folder(part) => return Err(unsafe_path()),
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(unsafe_path());
    }
    Ok(parts.join("/"))
}

pub fn is_git_folder(part: &str) -> bool {
    let ignored = |character: char| matches!(character, '\u{200C}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{206A}'..='\u{206F}' | '\u{FEFF}');
    let visible: String = part.chars().filter(|character| !ignored(*character)).collect();
    let name = visible.trim_end_matches([' ', '.']).to_ascii_lowercase();
    name == ".git" || name == "git~1"
}

impl Patch {
    pub fn detected_strip(&self) -> usize {
        let prefixed =
            |path: &Option<String>, prefix: &str| path.as_deref().is_none_or(|path| path.starts_with(prefix));
        let git = !self.files.is_empty()
            && self.files.iter().all(|file| prefixed(&file.old_path, "a/") && prefixed(&file.new_path, "b/"));
        usize::from(git)
    }

    pub fn strip(&self, count: usize) -> Result<Patch, PatchError> {
        self.map_paths(|path| {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() <= count {
                return Err(PatchError::Strip(path.to_owned()));
            }
            safe_path(&parts[count..].join("/"))
        })
    }

    pub fn without_git_prefixes(&self) -> Patch {
        let mut patch = self.clone();
        for file in &mut patch.files {
            for (path, prefix) in [(&mut file.old_path, "a/"), (&mut file.new_path, "b/")] {
                if let Some(stripped) = path.as_deref().and_then(|path| path.strip_prefix(prefix)) {
                    *path = Some(stripped.to_owned());
                }
            }
        }
        patch
    }

    pub fn under(&self, folder: &str) -> Result<Patch, PatchError> {
        let folder = folder.trim_matches('/');
        if folder.is_empty() {
            return Ok(self.clone());
        }
        self.map_paths(|path| safe_path(&format!("{folder}/{path}")))
    }

    pub fn relative_to(&self, folder: &str) -> Result<Patch, PatchError> {
        let folder = folder.trim_matches('/');
        if folder.is_empty() {
            return Ok(self.clone());
        }
        self.map_paths(|path| {
            path.strip_prefix(folder)
                .and_then(|rest| rest.strip_prefix('/'))
                .map(str::to_owned)
                .ok_or_else(|| PatchError::OutsideBase(path.to_owned()))
        })
    }

    fn map_paths(&self, map: impl Fn(&str) -> Result<String, PatchError>) -> Result<Patch, PatchError> {
        let mut patch = self.clone();
        for file in &mut patch.files {
            file.old_path = file.old_path.as_deref().map(&map).transpose()?;
            file.new_path = file.new_path.as_deref().map(&map).transpose()?;
        }
        Ok(patch)
    }

    pub fn reversed(&self) -> Patch {
        let mut patch = self.clone();
        for file in &mut patch.files {
            std::mem::swap(&mut file.old_path, &mut file.new_path);
            std::mem::swap(&mut file.old_mode, &mut file.new_mode);
            file.kind = match file.kind {
                FileKind::Added => FileKind::Deleted,
                FileKind::Deleted => FileKind::Added,
                other => other,
            };
            if file.kind == FileKind::Copied {
                file.kind = FileKind::Modified;
            }
            file.index = file.index.as_deref().map(|index| {
                let (ids, mode) = index.split_once(' ').map_or((index, None), |(ids, mode)| (ids, Some(mode)));
                let swapped = ids.split_once("..").map_or(ids.to_owned(), |(old, new)| format!("{new}..{old}"));
                mode.map_or(swapped.clone(), |mode| format!("{swapped} {mode}"))
            });
            for hunk in &mut file.hunks {
                std::mem::swap(&mut hunk.old_start, &mut hunk.new_start);
                std::mem::swap(&mut hunk.old_lines, &mut hunk.new_lines);
                let lines = std::mem::take(&mut hunk.lines)
                    .into_iter()
                    .map(|line| Line {
                        kind: match line.kind {
                            LineKind::Added => LineKind::Removed,
                            LineKind::Removed => LineKind::Added,
                            LineKind::Context => LineKind::Context,
                        },
                        text: line.text,
                    })
                    .collect();
                hunk.lines = order_removed_first(lines);
            }
            if let Some(Binary::Data(data)) = &mut file.binary {
                let blocks: Vec<Vec<Vec<u8>>> =
                    data.split(Vec::is_empty).filter(|block| !block.is_empty()).map(<[Vec<u8>]>::to_vec).collect();
                if blocks.len() == 2 {
                    let mut swapped = blocks[1].clone();
                    swapped.push(Vec::new());
                    swapped.extend(blocks[0].iter().cloned());
                    *data = swapped;
                }
            }
        }
        patch
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(mail) = &self.mail {
            write_mail(&mut out, mail);
        }
        for file in &self.files {
            write_file(&mut out, file);
        }
        out
    }
}

fn order_removed_first(lines: Vec<Line>) -> Vec<Line> {
    let mut ordered = Vec::with_capacity(lines.len());
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    for line in lines {
        match line.kind {
            LineKind::Context => {
                ordered.append(&mut removed);
                ordered.append(&mut added);
                ordered.push(line);
            }
            LineKind::Removed => removed.push(line),
            LineKind::Added => added.push(line),
        }
    }
    ordered.append(&mut removed);
    ordered.append(&mut added);
    ordered
}

fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        return value.to_owned();
    }
    let mut encoded = String::from("=?UTF-8?q?");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'!' | b'*' | b'+' | b'/') {
            encoded.push(char::from(byte));
        } else if byte == b' ' {
            encoded.push('_');
        } else {
            encoded.push_str(&format!("={byte:02X}"));
        }
    }
    encoded.push_str("?=");
    encoded
}

fn write_mail(out: &mut Vec<u8>, mail: &Mail) {
    let commit = mail.commit.clone().unwrap_or_else(|| "0".repeat(40));
    out.extend_from_slice(format!("From {commit} Mon Sep 17 00:00:00 2001\n").as_bytes());
    let author = match mail.author.rsplit_once(" <") {
        Some((name, email)) => format!("{} <{email}", encode_header(name)),
        None => encode_header(&mail.author),
    };
    out.extend_from_slice(format!("From: {author}\n").as_bytes());
    if !mail.date.is_empty() {
        out.extend_from_slice(format!("Date: {}\n", mail.date).as_bytes());
    }
    out.extend_from_slice(format!("Subject: [PATCH] {}\n", encode_header(mail.subject.trim())).as_bytes());
    if !mail.body.is_ascii() || !mail.subject.is_ascii() {
        out.extend_from_slice(
            b"MIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n",
        );
    }
    out.push(b'\n');
    if !mail.body.trim().is_empty() {
        out.extend_from_slice(mail.body.trim().as_bytes());
        out.extend_from_slice(b"\n\n");
    }
    out.extend_from_slice(b"---\n\n");
}

pub fn quote_path(path: &str) -> String {
    let special = |byte: u8| byte < 0x20 || byte == b'"' || byte == b'\\' || byte == 0x7f;
    if !path.bytes().any(special) {
        return path.to_owned();
    }
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            character if (character as u32) < 0x20 || character as u32 == 0x7f => {
                quoted.push_str(&format!("\\{:03o}", character as u32))
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn prefixed(prefix: &str, path: &str) -> String {
    quote_path(&format!("{prefix}{path}"))
}

fn write_file(out: &mut Vec<u8>, file: &FilePatch) {
    let old = file.old_path.as_deref().or(file.new_path.as_deref()).unwrap_or_default();
    let new = file.new_path.as_deref().or(file.old_path.as_deref()).unwrap_or_default();
    let mut line = |text: String| {
        out.extend_from_slice(text.as_bytes());
        out.push(b'\n');
    };
    line(format!("diff --git {} {}", prefixed("a/", old), prefixed("b/", new)));
    match file.kind {
        FileKind::Added => line(format!("new file mode {}", file.new_mode.as_deref().unwrap_or("100644"))),
        FileKind::Deleted => line(format!("deleted file mode {}", file.old_mode.as_deref().unwrap_or("100644"))),
        _ => {
            if let (Some(old_mode), Some(new_mode)) = (&file.old_mode, &file.new_mode)
                && old_mode != new_mode
            {
                line(format!("old mode {old_mode}"));
                line(format!("new mode {new_mode}"));
            }
        }
    }
    if matches!(file.kind, FileKind::Renamed | FileKind::Copied) {
        let verb = if file.kind == FileKind::Renamed { "rename" } else { "copy" };
        line(format!("similarity index {}", file.similarity.as_deref().unwrap_or("100%")));
        line(format!("{verb} from {}", quote_path(old)));
        line(format!("{verb} to {}", quote_path(new)));
    }
    if let Some(index) = &file.index {
        let ids = index.split(' ').next().unwrap_or_default();
        let same_mode = file.old_mode.is_some() && file.old_mode == file.new_mode;
        match (&file.kind, same_mode, &file.new_mode) {
            (FileKind::Modified | FileKind::Renamed | FileKind::Copied, true, Some(mode)) => {
                line(format!("index {ids} {mode}"))
            }
            _ => line(format!("index {ids}")),
        }
    }
    match &file.binary {
        Some(Binary::Data(data)) => {
            out.extend_from_slice(b"GIT binary patch\n");
            for data_line in data {
                out.extend_from_slice(data_line);
                out.push(b'\n');
            }
            out.push(b'\n');
            return;
        }
        Some(Binary::Marker) => {
            let old = if file.kind == FileKind::Added { "/dev/null".to_owned() } else { prefixed("a/", old) };
            let new = if file.kind == FileKind::Deleted { "/dev/null".to_owned() } else { prefixed("b/", new) };
            out.extend_from_slice(format!("Binary files {old} and {new} differ\n").as_bytes());
            return;
        }
        None => {}
    }
    if file.hunks.is_empty() {
        return;
    }
    let old_name = if file.kind == FileKind::Added { "/dev/null".to_owned() } else { prefixed("a/", old) };
    let new_name = if file.kind == FileKind::Deleted { "/dev/null".to_owned() } else { prefixed("b/", new) };
    out.extend_from_slice(format!("--- {old_name}\n+++ {new_name}\n").as_bytes());
    for hunk in &file.hunks {
        write_hunk(out, hunk);
    }
}

fn hunk_range(start: usize, count: usize) -> String {
    if count == 1 { start.to_string() } else { format!("{start},{count}") }
}

fn write_hunk(out: &mut Vec<u8>, hunk: &Hunk) {
    let old_count = hunk.lines.iter().filter(|line| line.kind != LineKind::Added).count();
    let new_count = hunk.lines.iter().filter(|line| line.kind != LineKind::Removed).count();
    out.extend_from_slice(
        format!("@@ -{} +{} @@", hunk_range(hunk.old_start, old_count), hunk_range(hunk.new_start, new_count))
            .as_bytes(),
    );
    out.extend_from_slice(&hunk.section);
    out.push(b'\n');
    for line in &hunk.lines {
        out.push(match line.kind {
            LineKind::Context => b' ',
            LineKind::Removed => b'-',
            LineKind::Added => b'+',
        });
        out.extend_from_slice(&line.text);
        if line.text.last() != Some(&b'\n') {
            out.extend_from_slice(b"\n\\ No newline at end of file\n");
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HunkResult {
    pub applied: bool,
    pub line: usize,
    pub offset: isize,
    pub fuzz: usize,
    pub exact: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub hunks: Vec<HunkResult>,
    pub theirs: Vec<u8>,
    pub base: Vec<u8>,
}

impl Applied {
    pub fn clean(&self) -> bool {
        self.hunks.iter().all(|hunk| hunk.applied)
    }
}

fn loose(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &line[..end]
}

fn matches_at(file: &[&[u8]], at: usize, pre: &[&[u8]], exact: bool) -> bool {
    if at + pre.len() > file.len() {
        return false;
    }
    pre.iter().zip(&file[at..at + pre.len()]).all(
        |(want, have)| {
            if exact { want == have } else { loose(want) == loose(have) }
        },
    )
}

struct Text<'a> {
    lines: Vec<&'a [u8]>,
    exact: OnceCell<HashMap<&'a [u8], Vec<usize>>>,
    loose: OnceCell<HashMap<&'a [u8], Vec<usize>>>,
}

impl<'a> Text<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Text { lines: split_lines(bytes), exact: OnceCell::new(), loose: OnceCell::new() }
    }

    fn crlf(&self) -> bool {
        let breaks = self.lines.iter().filter(|line| line.ends_with(b"\n")).count();
        let crlf = self.lines.iter().filter(|line| line.ends_with(b"\r\n")).count();
        crlf * 2 > breaks
    }

    fn starts_of(&self, line: &[u8], exact: bool) -> Option<&Vec<usize>> {
        let index = |key: fn(&'a [u8]) -> &'a [u8]| {
            let mut index: HashMap<&'a [u8], Vec<usize>> = HashMap::new();
            for (at, line) in self.lines.iter().enumerate() {
                index.entry(key(line)).or_default().push(at);
            }
            index
        };
        if exact {
            self.exact.get_or_init(|| index(|line| line)).get(line)
        } else {
            self.loose.get_or_init(|| index(loose)).get(loose(line))
        }
    }

    fn find(&self, pre: &[&[u8]], expected: usize, from: usize, exact: bool, anchor: Anchor) -> Option<usize> {
        let lines = &self.lines;
        if lines.len() < pre.len() {
            return None;
        }
        let last = lines.len() - pre.len();
        if from > last {
            return None;
        }
        let fixed = match (anchor.start, anchor.end) {
            (true, true) if last != 0 => return None,
            (true, _) => Some(0),
            (false, true) => Some(last),
            (false, false) => None,
        };
        if let Some(at) = fixed {
            return (at >= from && matches_at(lines, at, pre, exact)).then_some(at);
        }
        let expected = expected.clamp(from, last);
        let Some(first) = pre.first() else { return Some(expected) };
        let starts = self.starts_of(first, exact)?;
        let split = starts.partition_point(|start| *start < expected);
        let (mut up, mut down) = (split, split);
        loop {
            let above = starts.get(up).copied().filter(|start| *start <= last);
            let below = down.checked_sub(1).map(|index| starts[index]).filter(|start| *start >= from);
            let (at, going_up) = match (above, below) {
                (None, None) => return None,
                (Some(above), Some(below)) if above - expected <= expected - below => (above, true),
                (Some(above), None) => (above, true),
                (_, Some(below)) => (below, false),
            };
            if matches_at(lines, at, pre, exact) {
                return Some(at);
            }
            if going_up {
                up += 1;
            } else {
                down -= 1;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Anchor {
    start: bool,
    end: bool,
}

fn with_ending(text: &[u8], crlf: bool) -> std::borrow::Cow<'_, [u8]> {
    match (text.strip_suffix(b"\r\n"), text.strip_suffix(b"\n")) {
        (Some(body), _) if !crlf => [body, b"\n"].concat().into(),
        (None, Some(body)) if crlf => [body, b"\r\n"].concat().into(),
        _ => text.into(),
    }
}

pub fn apply(original: &[u8], file: &FilePatch) -> Applied {
    let text = Text::new(original);
    let lines = &text.lines;
    let crlf = text.crlf();
    let mut result = Applied::default();
    let mut base: Vec<u8> = Vec::with_capacity(original.len());
    let mut theirs: Vec<u8> = Vec::with_capacity(original.len());
    let mut taken = 0usize;
    let mut drift: isize = 0;
    for hunk in &file.hunks {
        let expected_raw = if hunk.old_lines == 0 { hunk.old_start } else { hunk.old_start.saturating_sub(1) };
        let expected = (expected_raw as isize).saturating_add(drift).max(0) as usize;
        let context_before = hunk.lines.iter().take_while(|line| line.kind == LineKind::Context).count();
        let context_after = hunk.lines.iter().rev().take_while(|line| line.kind == LineKind::Context).count();
        let at_end = hunk.lines.iter().any(|line| line.text.last() != Some(&b'\n'));
        let mut found = None;
        'search: for fuzz in 0..=2usize {
            let leading = context_before.min(fuzz);
            let trailing = if at_end { 0 } else { context_after.min(fuzz) };
            if fuzz > 0 && (leading + trailing == 0 || leading + trailing >= hunk.lines.len()) {
                break;
            }
            let trimmed = &hunk.lines[leading..hunk.lines.len() - trailing];
            let pre: Vec<&[u8]> =
                trimmed.iter().filter(|line| line.kind != LineKind::Added).map(|line| line.text.as_slice()).collect();
            let passes: &[(bool, bool)] = if fuzz == 0 {
                &[(true, true), (true, false), (false, false)]
            } else {
                &[(true, false), (false, false)]
            };
            for &(exact, anchored) in passes {
                let anchor =
                    Anchor { start: anchored && hunk.old_start <= 1, end: at_end || (anchored && context_after == 0) };
                if let Some(at) = text.find(&pre, expected + leading, taken, exact, anchor) {
                    found = Some((at, trimmed, pre.len(), fuzz, leading, exact, fuzz == 0 && exact && anchored));
                    break 'search;
                }
            }
        }
        match found {
            Some((at, trimmed, length, fuzz, leading, exact, as_git)) => {
                for line in &lines[taken..at] {
                    base.extend_from_slice(line);
                    theirs.extend_from_slice(line);
                }
                let mut cursor = at;
                for line in trimmed {
                    match line.kind {
                        LineKind::Context => {
                            let have = lines.get(cursor).copied().unwrap_or_default();
                            theirs.extend_from_slice(have);
                            base.extend_from_slice(have);
                            cursor += 1;
                        }
                        LineKind::Removed => {
                            base.extend_from_slice(lines.get(cursor).copied().unwrap_or_default());
                            cursor += 1;
                        }
                        LineKind::Added if exact => theirs.extend_from_slice(&line.text),
                        LineKind::Added => theirs.extend_from_slice(&with_ending(&line.text, crlf)),
                    }
                }
                taken = at + length;
                let start = at.saturating_sub(leading);
                drift = start as isize - expected_raw as isize;
                result.hunks.push(HunkResult { applied: true, line: start + 1, offset: drift, fuzz, exact: as_git });
            }
            None => {
                let pre = hunk.preimage();
                let at = expected.clamp(taken, lines.len());
                let end = (at + pre.len()).min(lines.len());
                for line in &lines[taken..at] {
                    base.extend_from_slice(line);
                    theirs.extend_from_slice(line);
                }
                for line in &pre {
                    base.extend_from_slice(line);
                }
                for line in hunk.postimage() {
                    theirs.extend_from_slice(line);
                }
                taken = end;
                result.hunks.push(HunkResult { applied: false, line: at + 1, offset: 0, fuzz: 0, exact: false });
            }
        }
    }
    for line in &lines[taken.min(lines.len())..] {
        base.extend_from_slice(line);
        theirs.extend_from_slice(line);
    }
    if result.clean() {
        result.base = original.to_vec();
    } else {
        result.base = base;
    }
    result.theirs = theirs;
    result
}

pub fn merge_with_markers(base: &[u8], ours: &[u8], theirs: &[u8], labels: (&str, &str)) -> Option<(Vec<u8>, usize)> {
    let (base, ours, theirs) =
        (std::str::from_utf8(base).ok()?, std::str::from_utf8(ours).ok()?, std::str::from_utf8(theirs).ok()?);
    let (ours_lines, theirs_lines) = (crate::diff::lines(ours), crate::diff::lines(theirs));
    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let mut conflicts = 0;
    let take = |out: &mut String, lines: &[&str], range: &std::ops::Range<usize>| {
        for line in lines.get(range.start..range.end.min(lines.len())).unwrap_or_default() {
            out.push_str(line);
        }
    };
    let close = |out: &mut String| {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    };
    for chunk in merge::chunks(base, ours, theirs) {
        match chunk.kind {
            ChunkKind::Same | ChunkKind::Ours | ChunkKind::Both => take(&mut out, &ours_lines, &chunk.ours),
            ChunkKind::Theirs => take(&mut out, &theirs_lines, &chunk.theirs),
            ChunkKind::Conflict => {
                conflicts += 1;
                close(&mut out);
                out.push_str(&format!("<<<<<<< {}\n", labels.0));
                take(&mut out, &ours_lines, &chunk.ours);
                close(&mut out);
                out.push_str("=======\n");
                take(&mut out, &theirs_lines, &chunk.theirs);
                close(&mut out);
                out.push_str(&format!(">>>>>>> {}\n", labels.1));
            }
        }
    }
    Some((out.into_bytes(), conflicts))
}

pub fn decode(bytes: &[u8], encoding: &str) -> Option<Vec<u8>> {
    let encoding = encoding_rs::Encoding::for_label(encoding.trim().as_bytes())?;
    if encoding == encoding_rs::UTF_8 {
        return Some(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes).to_vec());
    }
    let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
    if had_errors {
        return Some(bytes.to_vec());
    }
    Some(text.into_owned().into_bytes())
}

pub fn encode(bytes: &[u8], encoding: &str) -> Option<Vec<u8>> {
    let encoding = encoding_rs::Encoding::for_label(encoding.trim().as_bytes())?;
    if encoding == encoding_rs::UTF_8 {
        return Some(bytes.to_vec());
    }
    let mut out = Vec::with_capacity(bytes.len());
    for line in split_lines(bytes) {
        match std::str::from_utf8(line) {
            Ok(text) => {
                let (encoded, _, unmappable) = encoding.encode(text);
                if unmappable {
                    out.extend_from_slice(line);
                } else {
                    out.extend_from_slice(&encoded);
                }
            }
            Err(_) => out.extend_from_slice(line),
        }
    }
    Some(out)
}

pub fn mail_date(time: i64, offset: i32) -> String {
    let local = time.saturating_add(i64::from(offset));
    let days = local.div_euclid(86_400);
    let seconds = local.rem_euclid(86_400);
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let name = names[((month - 1).clamp(0, 11)) as usize];
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.unsigned_abs() / 60;
    format!(
        "{weekday}, {day} {name} {year} {:02}:{:02}:{:02} {sign}{:02}{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60,
        minutes / 60,
        minutes % 60
    )
}

pub fn encoding_name(label: &str) -> Option<&'static str> {
    encoding_rs::Encoding::for_label(label.trim().as_bytes()).map(encoding_rs::Encoding::name)
}

#[cfg(test)]
mod tests;
