pub mod client;
pub mod connection;
#[cfg(feature = "fake")]
pub mod fake;
pub mod protocol;
pub mod reverse;

pub use client::{Client, Configuration, Handshake, SourceMap};
pub use connection::{Connection, Incoming, Pending, Waker};
pub use protocol::{
    Ack, Breakpoint, Capabilities, DataBreakpoint, DataBreakpointInfoResponse, DebugEvent, EvaluateResponse,
    ExceptionBreakpointsFilter, ExceptionFilterOptions, FunctionBreakpoint, Scope, SetExceptionBreakpointsArguments,
    SetValueResponse, Source, SourceBreakpoint, StackFrame, StackTraceResponse, StepInTarget, StoppedEvent, Thread,
    Variable,
};
pub use reverse::ReverseListener;

pub const LOCALES: &[(&str, &str)] =
    &[("en", include_str!("../locales/en/dap.ftl")), ("ru", include_str!("../locales/ru/dap.ftl"))];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DapError {
    Io(String),
    Closed,
    Timeout,
    Adapter { message: String },
}

impl std::fmt::Display for DapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot talk to the debugger: {error}"),
            Self::Closed => f.write_str("the debugger is not running"),
            Self::Timeout => f.write_str("the debugger did not answer in time"),
            Self::Adapter { message } => write!(f, "the debugger answered: {message}"),
        }
    }
}

impl std::error::Error for DapError {}
