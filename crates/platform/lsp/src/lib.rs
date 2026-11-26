pub mod client;
pub mod connection;
#[cfg(feature = "fake")]
pub mod fake;
pub mod file_operations;
pub mod log;
pub mod outbox;
pub mod position;
pub mod servers;

pub use client::{Client, DocumentRequest, Edit, Event, Launched, Launcher, logged_process_launcher, process_launcher};
pub use connection::{Connection, Incoming, Pending, ServerRequest};
pub use file_operations::FileOperation;
pub use log::ServerLog;
pub use lsp_types;
pub use servers::{
    ExtraFile, GenerateDeclaration, InstallKind, InstallRecipe, LaunchMissing, LaunchPlaces, MemberList,
    NotInstallable, PromptGenerator, PromptOption, ServerDeclaration, ServerPlugin, ServerTools, SystemTools,
    VirtualDocuments,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LspError {
    Io(String),
    Closed,
    Timeout,
    Server { code: i64, message: String },
    NotOpen(String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot talk to the language server: {error}"),
            Self::Closed => f.write_str("the language server is not running"),
            Self::Timeout => f.write_str("the language server did not answer in time"),
            Self::Server { code, message } => write!(f, "the language server answered with error {code}: {message}"),
            Self::NotOpen(uri) => write!(f, "{uri} is not open in the language server"),
        }
    }
}

impl std::error::Error for LspError {}
