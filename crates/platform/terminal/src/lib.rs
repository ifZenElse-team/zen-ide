use std::fmt;
use std::path::PathBuf;

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub mod host;
pub mod keys;
pub mod links;
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod replay;
mod screen;
#[cfg(not(target_arch = "wasm32"))]
mod session;
#[cfg(target_arch = "wasm32")]
mod stub;

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub use host::Host;
#[cfg(not(all(unix, not(target_arch = "wasm32"))))]
mod no_host;
#[cfg(not(all(unix, not(target_arch = "wasm32"))))]
pub use no_host::Host;
pub use screen::{Attributes, Cell, Color, Cursor, GridPoint, Screen, SelectionKind, tidy_copied};
#[cfg(not(target_arch = "wasm32"))]
pub use session::Session;
#[cfg(target_arch = "wasm32")]
pub use stub::Session;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

impl Size {
    pub const MIN: Size = Size { rows: 1, cols: 2 };

    pub const fn new(rows: u16, cols: u16) -> Self {
        Size { rows, cols }
    }

    pub fn clamped(self) -> Self {
        Size { rows: self.rows.max(Self::MIN.rows), cols: self.cols.max(Self::MIN.cols) }
    }
}

impl Default for Size {
    fn default() -> Self {
        Size::new(24, 80)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub program: Option<PathBuf>,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub size: Size,
    pub scrollback: usize,
    pub env: Vec<(String, String)>,
    pub keep: bool,
    pub preface: String,
}

impl Options {
    pub const DEFAULT_SCROLLBACK: usize = 10_000;

    pub fn shell(cwd: impl Into<PathBuf>) -> Self {
        Options {
            program: None,
            args: Vec::new(),
            cwd: cwd.into(),
            size: Size::default(),
            scrollback: Self::DEFAULT_SCROLLBACK,
            env: Vec::new(),
            keep: false,
            preface: String::new(),
        }
    }

    pub fn command(program: impl Into<PathBuf>, args: &[&str], cwd: impl Into<PathBuf>) -> Self {
        Options {
            program: Some(program.into()),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..Options::shell(cwd)
        }
    }
}

pub fn default_shell() -> PathBuf {
    match std::env::var_os("SHELL") {
        Some(shell) if !shell.is_empty() => PathBuf::from(shell),
        _ if cfg!(target_os = "macos") => PathBuf::from("/bin/zsh"),
        _ => PathBuf::from("/bin/bash"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostedSession {
    pub id: u64,
    pub protocol: u32,
    pub host: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
}

#[derive(Debug)]
pub enum Error {
    Spawn(std::io::Error),
    Incompatible { protocol: u32 },
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Spawn(error) => write!(f, "cannot start the terminal: {error}"),
            Error::Incompatible { protocol } => {
                write!(f, "the terminal runs in a host that speaks protocol {protocol}, which this IDE does not read")
            }
            Error::Unsupported => f.write_str("terminals are not supported in this build"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Spawn(error) => Some(error),
            Error::Incompatible { .. } | Error::Unsupported => None,
        }
    }
}
