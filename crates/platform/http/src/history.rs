use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Header, Response};

pub const LIMIT: usize = 50;

pub const BODY_KEPT: usize = 1 << 20;

pub const BODIES_KEPT: usize = 16 << 20;

/// Where a request is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeState {
    /// Sent; the answer has not come yet.
    Running,
    /// Answered, whatever the status.
    Done,
    /// Not sent or not answered: `error` says why.
    Failed,
}

/// A response as the history keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoredResponse {
    /// The status code.
    pub status: u16,
    /// The status code's reason phrase.
    pub reason: String,
    /// The protocol version.
    pub version: String,
    /// The headers in the order they came.
    pub headers: Vec<Header>,
    /// The `Content-Type` header.
    pub content_type: Option<String>,
    /// The body as text, at most [`BODY_KEPT`] bytes; empty for a binary body.
    pub body: String,
    /// The body's size in bytes.
    pub size: u64,
    /// Whether the body was cut to [`BODY_KEPT`] bytes.
    pub truncated: bool,
    /// Whether the body is not UTF-8 text.
    pub binary: bool,
}

pub fn cut(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_owned(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

impl StoredResponse {
    pub fn of(response: &Response) -> Self {
        let content_type = response
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.clone());
        let (body, truncated, binary) = match std::str::from_utf8(&response.body) {
            Ok(text) => {
                let (body, truncated) = cut(text, BODY_KEPT);
                (body, truncated, false)
            }
            Err(_) => (String::new(), false, true),
        };
        Self {
            status: response.status,
            reason: response.reason.clone(),
            version: response.version.clone(),
            headers: response
                .headers
                .iter()
                .map(|(name, value)| Header { name: name.clone(), value: value.clone() })
                .collect(),
            content_type,
            body,
            size: response.body.len() as u64,
            truncated,
            binary,
        }
    }
}

/// One request that was run and what came back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Exchange {
    /// Its id, for `http.response.get`.
    pub id: u64,
    /// The request file.
    pub file: PathBuf,
    /// The request's place in the file, from 0.
    pub index: usize,
    /// The request's name, when it has one.
    pub name: Option<String>,
    /// The environment it was run with.
    pub environment: Option<String>,
    /// The method.
    pub method: String,
    /// The URL with its variables resolved.
    pub url: String,
    /// The headers sent, as written with their variables resolved.
    pub request_headers: Vec<Header>,
    /// The body sent, as text.
    pub request_body: Option<String>,
    /// When it was sent, in milliseconds since 1970.
    pub started: u64,
    /// Where it is.
    pub state: ExchangeState,
    /// How long the exchange took, in milliseconds.
    pub duration_ms: Option<u64>,
    /// The answer.
    pub response: Option<StoredResponse>,
    /// Why it failed.
    pub error: Option<String>,
    /// The files its body read (`< file`), by their real paths: an agent gets neither the body nor
    /// the answer of a request that read a file hidden from agents (ADR-0143).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_files: Vec<PathBuf>,
}

impl Exchange {
    fn body_bytes(&self) -> usize {
        self.request_body.as_ref().map_or(0, String::len)
            + self.response.as_ref().map_or(0, |response| response.body.len())
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    exchanges: Vec<Exchange>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    file: Option<PathBuf>,
    exchanges: Vec<Exchange>,
}

impl History {
    pub fn load(file: Option<PathBuf>) -> Self {
        let exchanges = file
            .as_deref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
            .map(|stored| stored.exchanges)
            .unwrap_or_default();
        Self { file, exchanges }
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    pub fn get(&self, id: u64) -> Option<&Exchange> {
        self.exchanges.iter().find(|exchange| exchange.id == id)
    }

    pub fn last_id(&self) -> u64 {
        self.exchanges.iter().map(|exchange| exchange.id).max().unwrap_or(0)
    }

    pub fn put(&mut self, exchange: Exchange) {
        match self.exchanges.iter_mut().find(|known| known.id == exchange.id) {
            Some(known) => *known = exchange,
            None => self.exchanges.insert(0, exchange),
        }
        self.exchanges.truncate(LIMIT);
        let mut total = 0;
        let kept = self
            .exchanges
            .iter()
            .position(|exchange| {
                total += exchange.body_bytes();
                total > BODIES_KEPT
            })
            .map_or(self.exchanges.len(), |over| over.max(1));
        self.exchanges.truncate(kept);
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(file) = &self.file else { return Ok(()) };
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let stored = Stored {
            exchanges: self
                .exchanges
                .iter()
                .filter(|exchange| exchange.state != ExchangeState::Running)
                .cloned()
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&stored).map_err(std::io::Error::other)?;
        let temporary = file.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, file)
    }
}

pub fn history_file(data_dir: &Path, project: &Path) -> PathBuf {
    let name: String = project
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in project.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    data_dir.join("http-client").join(format!("{name}-{hash:016x}")).join("history.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(id: u64, state: ExchangeState) -> Exchange {
        Exchange {
            id,
            file: PathBuf::from("/p/api.http"),
            index: 0,
            name: None,
            environment: Some("dev".into()),
            method: "GET".into(),
            url: "http://127.0.0.1/".into(),
            request_headers: Vec::new(),
            request_body: None,
            started: 1,
            state,
            duration_ms: Some(3),
            response: None,
            error: None,
            body_files: Vec::new(),
        }
    }

    #[test]
    fn keeps_the_newest_and_saves_ended_ones() {
        let data = tempfile::tempdir().unwrap();
        let file = history_file(data.path(), Path::new("/work/my project"));
        assert!(file.starts_with(data.path().join("http-client")));
        assert!(file.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("my_project-"));
        assert_ne!(file, history_file(data.path(), Path::new("/other/my project")));
        let mut history = History::load(Some(file.clone()));
        assert!(history.exchanges().is_empty());
        for id in 1..=LIMIT as u64 + 2 {
            history.put(exchange(id, ExchangeState::Done));
        }
        history.put(exchange(100, ExchangeState::Running));
        assert_eq!(history.exchanges().len(), LIMIT);
        assert_eq!(history.exchanges()[0].id, 100);
        history.save().unwrap();
        let read = History::load(Some(file.clone()));
        assert_eq!(read.exchanges().len(), LIMIT - 1, "a running exchange is not saved");
        assert_eq!(read.exchanges()[0].id, LIMIT as u64 + 2);
        assert_eq!(read.last_id(), LIMIT as u64 + 2);
        std::fs::write(&file, "broken").unwrap();
        assert!(History::load(Some(file)).exchanges().is_empty());
    }

    #[test]
    fn bodies_are_bounded_in_all() {
        let mut history = History::load(None);
        let big = |id| Exchange { request_body: Some("x".repeat(BODY_KEPT)), ..exchange(id, ExchangeState::Done) };
        for id in 1..=20 {
            history.put(big(id));
        }
        assert_eq!(history.exchanges().len(), BODIES_KEPT / BODY_KEPT);
        assert_eq!(history.exchanges()[0].id, 20);
        let mut huge = History::load(None);
        huge.put(Exchange { request_body: Some("x".repeat(BODIES_KEPT + 1)), ..exchange(1, ExchangeState::Done) });
        assert_eq!(huge.exchanges().len(), 1, "the newest stays whatever its size");
        assert_eq!(cut("éé", 3), ("é".to_owned(), true));
        assert_eq!(cut("ab", 3), ("ab".to_owned(), false));
    }

    #[test]
    fn bodies_are_kept_as_text_cut_or_marked_binary() {
        let response = |body: Vec<u8>| Response {
            status: 200,
            reason: "OK".into(),
            version: "HTTP/1.1".into(),
            headers: vec![("content-type".into(), "text/plain".into())],
            body,
            duration_ms: 1,
        };
        let text = StoredResponse::of(&response(b"hello".to_vec()));
        assert_eq!((text.body.as_str(), text.size, text.truncated, text.binary), ("hello", 5, false, false));
        assert_eq!(text.content_type.as_deref(), Some("text/plain"));
        let long = StoredResponse::of(&response("é".repeat(BODY_KEPT).into_bytes()));
        assert!(long.truncated && long.body.len() <= BODY_KEPT && long.size == 2 * BODY_KEPT as u64);
        let binary = StoredResponse::of(&response(vec![0xff, 0xfe, 0]));
        assert!(binary.binary && binary.body.is_empty());
    }
}
