pub mod env;
pub mod history;
pub mod parse;
pub mod pretty;
pub mod vars;

#[cfg(not(target_arch = "wasm32"))]
mod send;
#[cfg(any(test, feature = "test-server"))]
pub mod test_server;

pub use env::{EnvError, Environments, PRIVATE_FILE, PUBLIC_FILE};
pub use history::{Exchange, ExchangeState, History, StoredResponse, cut, history_file};
pub use parse::{Header, Request, is_http_file, parse};
pub use pretty::{pretty_body, pretty_json};
pub use vars::{PrepareError, Prepared, Shown, Variables, prepare};

#[cfg(not(target_arch = "wasm32"))]
pub use send::send;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    Failed(String),
    RedirectScheme(String),
    TooManyRedirects,
    Timeout,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(error) => f.write_str(error),
            Self::RedirectScheme(scheme) => write!(f, "a redirect to a {scheme}: address is not followed"),
            Self::TooManyRedirects => f.write_str("too many redirects"),
            Self::Timeout => f.write_str("timeout"),
        }
    }
}

impl std::error::Error for SendError {}
