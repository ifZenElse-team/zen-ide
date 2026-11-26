use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::sync::OwnedMutexGuard;
use tokio_util::sync::CancellationToken;

use crate::Waker;
use crate::driver::{
    Canceller, ConnectSpec, ContextKind, DbError, Driver, DriverSession, ErrorKind, ExecOptions, Guards, TrackedState,
};
use crate::model::Node;
use crate::registry::{DataSource, POOLER};
use crate::schema::{IntrospectRequest, Introspected, Snapshot};
use crate::types::ConnectionType;
use crate::value::Execution;
use zen_secrets::{Secret, SecretError, SecretId, SecretStore};

mod compare;
mod csv_import;
mod explain;
mod export;
mod script;
mod search;
mod sessions;
mod transactions;
mod transfer;
mod tunnels;

pub use compare::{ComparePass, DataCompareRequest, DataCompareState, DataSide, KEPT_ROWS, PAGE as COMPARE_PAGE};
pub use csv_import::{ImportRequest, ImportState, RowError};
pub use explain::ExplainRun;
pub use export::{ExportItem, ExportItemState, ExportState, ItemStatus, Output, TEXT_LIMIT, now_stamps};
pub use script::{Message, QueryOptions, QueryState, QueryStatement, StatementState, StatementStatus};
pub use search::{ColumnHit, SearchRequest, SearchState, TableHit, TableStatus};
pub use transactions::{CommitMode, LoggedStatement, ModeChange, TransactionState};
pub use transfer::{TransferRequest, TransferState, TransferTable, TransferTableState};
pub use tunnels::SshPrompt;

mod save;

const KEPT_OPERATIONS: usize = 200;

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub fn runtime() -> &'static Handle {
    static HANDLE: OnceLock<Handle> = OnceLock::new();
    HANDLE.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new().name("zen-db".to_owned()).spawn(move || {
            let runtime =
                tokio::runtime::Builder::new_current_thread().enable_all().thread_name("zen-db-blocking").build();
            match runtime {
                Ok(runtime) => {
                    let _ = sender.send(Some(runtime.handle().clone()));
                    runtime.block_on(std::future::pending::<()>());
                }
                Err(_) => {
                    let _ = sender.send(None);
                }
            }
        });
        started.ok().and_then(|_| receiver.recv().ok().flatten()).expect("the zen-db runtime starts")
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    pub connect_timeout: Duration,
    pub ping_after: Duration,
    pub ping_timeout: Duration,
    pub backoff: Vec<Duration>,
    pub cancel_wait: Duration,
    pub tick: Duration,
    pub sleep_gap: Duration,
    pub metadata_lock_timeout: Duration,
    pub metadata_idle_transaction: Duration,
    pub idle_countdown: Duration,
    pub secret_timeout: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            ping_after: Duration::from_secs(30),
            ping_timeout: Duration::from_secs(5),
            backoff: [0, 250, 500, 1000, 2000, 4000].map(Duration::from_millis).to_vec(),
            cancel_wait: Duration::from_secs(1),
            tick: Duration::from_secs(5),
            sleep_gap: Duration::from_secs(30),
            metadata_lock_timeout: Duration::from_secs(10),
            metadata_idle_transaction: Duration::from_secs(60),
            idle_countdown: Duration::from_secs(10),
            secret_timeout: Duration::from_secs(30),
        }
    }
}

/// What an operation does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    /// Opening a connection's Main and Metadata contexts.
    Connect,
    /// Testing settings with a connection of its own.
    Test,
    /// Closing a connection.
    Disconnect,
    /// Closing and opening a connection again.
    Invalidate,
    /// Running a statement.
    Execute,
    /// Reading nodes of the driver's tree.
    Children,
    /// Setting tracked session state.
    Apply,
    /// Storing a connection's secret in the secret store (the Keychain).
    StoreSecret,
    /// Deleting a connection's secrets from the secret store.
    DeleteSecrets,
    /// Running the statements of a query or script.
    Query,
    /// Reading a result on past where it stopped.
    Fetch,
    /// Counting the rows of a result's query.
    Count,
    /// Reading the structure into a snapshot.
    Introspect,
    /// Saving a result's staged changes.
    Save,
    /// Committing a context's transaction.
    Commit,
    /// Rolling back a context's transaction.
    Rollback,
    /// Changing a context's commit mode or isolation.
    Mode,
    /// Closing one context's session, as closing a console does.
    Close,
    /// Reading or changing one key of a key-value database.
    Key,
    /// Exporting rows to files or text.
    Export,
    /// Explaining a statement's plan.
    Explain,
    /// Importing a CSV file into a table.
    Import,
    /// Listing the server's sessions and locks.
    Sessions,
    /// Cancelling a server session's statement or ending the session.
    EndSession,
    /// Finding text in the tables of a schema.
    Search,
    /// Comparing the rows of two tables.
    Compare,
    /// Reading a connection's saved password for a client program of its own.
    ReadSecret,
    /// Copying tables from one connection into another.
    Transfer,
}

/// Where an operation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    /// Running.
    Running,
    /// Finished.
    Done,
    /// Failed; `error` says why.
    Failed,
    /// Cancelled.
    Cancelled,
}

/// An operation as agents and views read it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationState {
    /// Its id.
    pub id: u64,
    /// What it does.
    pub kind: OperationKind,
    /// The connection it works on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The context it runs in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextKind>,
    /// Where it is.
    pub status: OperationStatus,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// Why it failed or was cancelled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
    /// The statement's result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<Execution>,
    /// The nodes read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<Node>>,
    /// What a connection test found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<TestReport>,
    /// The key read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<crate::value::KeyValue>,
    /// What an introspection read; the snapshot itself is taken with `take_snapshot`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introspected: Option<Introspected>,
    /// Whether the connection was opened again during it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reconnected: bool,
    /// What saving a result's changes did, also when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<SaveReport>,
    /// The server's sessions and locks listed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<crate::sessions::ServerSessions>,
    /// What ending a server session did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<crate::sessions::Ended>,
}

/// What saving a result's staged changes did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    /// The statements run before it ended.
    pub statements: usize,
    /// Rows deleted.
    pub deleted: u64,
    /// Rows inserted.
    pub inserted: u64,
    /// Rows updated.
    pub updated: u64,
    /// The result row whose statement failed; every change was rolled back unless `kept`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_row: Option<usize>,
    /// Whether the failure is a conflict: the row was changed or deleted by someone else (0 rows changed).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conflict: bool,
    /// Whether the changes stay in the person's open transaction, to be committed or rolled back there.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub in_transaction: bool,
    /// Whether changes saved before a failure stay, because the database saves them without a transaction.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
}

/// What a connection test found: the server and how quickly it answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    /// The server's name and version, such as `PostgreSQL 18.0`, when it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    /// Milliseconds to open the session (TCP, TLS, authentication).
    pub connect_ms: u64,
    /// Milliseconds of one round trip to the server after connecting.
    pub round_trip_ms: u64,
    /// The database profile the server turned out to be (`cockroachdb`), when it is not the driver's own server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// What a disconnect does with open transactions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransactionPolicy {
    /// Refuses while a transaction is open, naming the contexts, so that the person decides.
    #[default]
    Ask,
    /// Commits them first.
    Commit,
    /// Rolls them back first.
    Rollback,
}

/// Why a connection closed by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CloseReason {
    /// It was idle longer than its connection type allows.
    Idle,
    /// The computer slept.
    Sleep,
}

/// The state of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    /// Not connected.
    Disconnected,
    /// Connecting.
    Connecting,
    /// Connected.
    Connected,
    /// Closed by itself; opened again on next use.
    Closed,
    /// The last connect failed.
    Failed,
}

/// One execution context of a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextStatus {
    /// Which context.
    pub context: ContextKind,
    /// Whether its session is open.
    pub open: bool,
    /// Whether a transaction is open in it.
    pub in_transaction: bool,
    /// Whether an operation runs in it.
    pub busy: bool,
    /// Seconds since it was last used.
    pub idle_seconds: u64,
}

/// What a connecting connection waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Waiting {
    /// The saved password from the Keychain, which may wait for the system's prompt.
    Keychain,
    /// The SSH tunnel to open.
    Tunnel,
    /// The database server to answer.
    Network,
}

/// A connection's status, never with a secret.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    /// The connection's id.
    pub connection: String,
    /// Its state.
    pub state: ConnectionState,
    /// Why it closed by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_because: Option<CloseReason>,
    /// Why the last connect failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
    /// The running connect operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
    /// What connecting waits for now, while it connects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<Waiting>,
    /// Seconds since it connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_seconds: Option<u64>,
    /// Whether statements commit on their own in its Main context.
    pub auto_commit: bool,
    /// Whether it is behind a transaction pooler.
    pub pooler: bool,
    /// The database profile its server turned out to be, when that is not the connection's own profile; the stored settings are left as they are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_profile: Option<String>,
    /// Its execution contexts.
    pub contexts: Vec<ContextStatus>,
}

impl ConnectionStatus {
    pub fn disconnected(id: &str) -> Self {
        Self {
            connection: id.to_owned(),
            state: ConnectionState::Disconnected,
            closed_because: None,
            error: None,
            operation: None,
            waiting_for: None,
            connected_seconds: None,
            auto_commit: true,
            pooler: false,
            detected_profile: None,
            contexts: Vec::new(),
        }
    }
}

/// Something that happened by itself, for a notification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Event {
    /// A connection closed after being idle.
    IdleClosed { connection: String, name: String },
    /// Connections closed because the computer slept; they open again on next use.
    Slept {
        /// How many connections closed.
        count: usize,
    },
    /// A transaction has been idle as long as the type allows; it is rolled back after `seconds` unless kept open.
    TransactionIdle {
        /// The connection's id.
        connection: String,
        /// Its name.
        name: String,
        /// The context it is in.
        context: ContextKind,
        /// Seconds until it is rolled back.
        seconds: u64,
    },
    /// A transaction idle longer than the type allows was rolled back.
    TransactionEnded { connection: String, name: String, context: ContextKind },
    /// A statement that changes the structure ran, so the connection's snapshot is read again.
    StructureChanged { connection: String },
    /// ssh asks a question while a connection's tunnel opens; answer it with `answer_ssh`.
    SshPrompt { prompt: SshPrompt },
    /// A lost session was opened again and its tracked state restored.
    Reconnected { connection: String, context: ContextKind },
    /// The server of a connection turned out to be another database profile than the connection names; its settings are left as they are.
    ProfileDetected { connection: String, name: String, profile: String },
    /// Storing (`stored`) or deleting a connection's secret failed; `detail` says why, never the
    /// secret.
    SecretFailed {
        /// The connection's id.
        connection: String,
        /// Whether it was a store (the connection's saved-password mark is then wrong).
        stored: bool,
        /// What the secret store said.
        detail: String,
    },
}

#[derive(Clone)]
pub struct StoredSecret {
    pub store: Arc<dyn SecretStore>,
    pub id: SecretId,
}

impl std::fmt::Debug for StoredSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StoredSecret({})", self.id)
    }
}

impl StoredSecret {
    async fn read(self, order: Arc<tokio::sync::Mutex<()>>, wait: Duration) -> Result<Secret, DbError> {
        let read = tokio::time::timeout(wait, async move {
            let _order = order.lock_owned().await;
            tokio::task::spawn_blocking(move || self.store.get(&self.id)).await
        })
        .await
        .map_err(|_| DbError::new(ErrorKind::PasswordNeeded, "").with_code("secretTimeout"))?
        .map_err(|error| DbError::new(ErrorKind::PasswordNeeded, error.to_string()))?;
        match read {
            Ok(Some(secret)) => Ok(secret),
            Ok(None) => Err(DbError::new(ErrorKind::PasswordNeeded, "")),
            Err(SecretError::Denied) => Err(DbError::new(ErrorKind::PasswordNeeded, "").with_code("secretDenied")),
            Err(error) => Err(DbError::new(ErrorKind::PasswordNeeded, error.to_string()).with_code("secretFailed")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub source: DataSource,
    pub kind: ConnectionType,
    pub password: Option<Secret>,
    pub stored: Option<StoredSecret>,
    pub ssh_secret: Option<StoredSecret>,
}

impl ConnectRequest {
    fn tracked(&self, context: &ContextKind) -> TrackedState {
        let pooler = self.source.flag(POOLER);
        let auto_commit = self.source.overrides.auto_commit.unwrap_or(self.kind.auto_commit);
        TrackedState {
            database: None,
            schema: self.source.bootstrap.default_schema.clone(),
            isolation: self.source.bootstrap.isolation.clone().filter(|_| *context != ContextKind::Metadata),
            auto_commit: pooler || *context == ContextKind::Metadata || auto_commit,
            read_only: self.source.read_only || self.kind.read_only || *context == ContextKind::Metadata,
        }
    }

    fn tracked_with(
        &self,
        context: &ContextKind,
        modes: &HashMap<(String, ContextKind), transactions::ModeSetting>,
    ) -> TrackedState {
        let mut tracked = self.tracked(context);
        if *context == ContextKind::Metadata || self.source.flag(POOLER) {
            return tracked;
        }
        if let Some(setting) = modes.get(&(self.source.id.clone(), context.clone())) {
            if let Some(mode) = setting.mode {
                tracked.auto_commit = mode != CommitMode::Manual;
            }
            if let Some(isolation) = &setting.isolation {
                tracked.isolation.clone_from(isolation);
            }
        }
        tracked
    }

    fn statement_timeout(&self) -> Option<Duration> {
        let seconds = self.source.overrides.statement_timeout.or(self.kind.statement_timeout)?;
        (seconds > 0).then(|| Duration::from_secs(seconds))
    }

    fn close_idle_after(&self) -> Option<Duration> {
        let seconds = self.source.overrides.close_idle_after.or(self.kind.close_idle_after)?;
        (seconds > 0).then(|| Duration::from_secs(seconds))
    }

    fn end_idle_transactions_after(&self) -> Option<Duration> {
        let seconds = self.source.overrides.end_idle_transactions_after.or(self.kind.end_idle_transactions_after)?;
        (seconds > 0).then(|| Duration::from_secs(seconds))
    }
}

struct Ctx {
    session: Option<Box<dyn DriverSession>>,
    tracked: TrackedState,
    last_used: Instant,
    opened_once: bool,
}

type CtxCell = Arc<tokio::sync::Mutex<Ctx>>;

#[derive(Clone, Debug, Default)]
struct CtxInfo {
    open: bool,
    in_transaction: bool,
    busy: usize,
    last_used: Option<Instant>,
    log: Vec<transactions::Logged>,
    logged: usize,
    modifying: usize,
    began: Option<Instant>,
    ending: Option<(Instant, CancellationToken)>,
    smart_manual: bool,
    server_id: Option<String>,
}

type TunnelSlot = Arc<tokio::sync::Mutex<Option<(crate::ssh::Tunnel, String)>>>;

struct Conn {
    tunnel: TunnelSlot,
    request: ConnectRequest,
    driver: Arc<dyn Driver>,
    state: ConnectionState,
    closed_because: Option<CloseReason>,
    error: Option<DbError>,
    connect_op: Option<u64>,
    waiting: Option<Waiting>,
    connected_at: Option<Instant>,
    contexts: BTreeMap<ContextKind, CtxCell>,
    info: BTreeMap<ContextKind, CtxInfo>,
    last_used: Instant,
    detected: Option<String>,
}

struct Op {
    state: OperationState,
    started: Instant,
    token: CancellationToken,
}

#[derive(Default)]
struct State {
    next: u64,
    operations: BTreeMap<u64, Op>,
    connections: HashMap<String, Conn>,
    events: Vec<Event>,
    snapshots: HashMap<u64, Snapshot>,
    plans: HashMap<u64, crate::explain::Plan>,
    monitoring: bool,
    queries: BTreeMap<u64, QueryState>,
    next_result: u64,
    modes: HashMap<(String, ContextKind), transactions::ModeSetting>,
    exports: BTreeMap<u64, Arc<export::Job>>,
    imports: BTreeMap<u64, Arc<csv_import::Job>>,
    searches: BTreeMap<u64, Arc<search::Job>>,
    compares: BTreeMap<u64, Arc<compare::Job>>,
    transfers: BTreeMap<u64, Arc<transfer::Job>>,
}

struct Shared {
    #[cfg_attr(not(unix), allow(dead_code))]
    id: u64,
    ssh: Mutex<crate::ssh::SshConfig>,
    askpass: Mutex<Option<Arc<tunnels::Askpass>>>,
    drivers: Vec<Arc<dyn Driver>>,
    more_drivers: Mutex<Option<MoreDrivers>>,
    application_name: String,
    timing: Timing,
    state: Mutex<State>,
    waker: Mutex<Option<Waker>>,
    secrets: Arc<tokio::sync::Mutex<()>>,
    results: Arc<crate::results::ResultStore>,
    history: Mutex<Option<Arc<dyn crate::history::HistorySink>>>,
}

pub type MoreDrivers = Arc<dyn Fn() -> Vec<Arc<dyn Driver>> + Send + Sync>;

#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
}

enum Outcome {
    Nothing,
    Execution(Execution),
    Nodes(Vec<Node>),
    Test(TestReport),
    Key(Box<crate::value::KeyValue>),
    Snapshot(Box<Snapshot>, Introspected),
    Plan(Box<crate::explain::Plan>),
    Sessions(Box<crate::sessions::ServerSessions>),
    Ended(crate::sessions::Ended),
}

impl Engine {
    pub fn new(drivers: Vec<Arc<dyn Driver>>, application_name: impl Into<String>) -> Self {
        Self::with_timing(drivers, application_name, Timing::default())
    }

    pub fn with_timing(drivers: Vec<Arc<dyn Driver>>, application_name: impl Into<String>, timing: Timing) -> Self {
        static ENGINES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        Self {
            shared: Arc::new(Shared {
                id: ENGINES.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                ssh: Mutex::new(crate::ssh::SshConfig::default()),
                askpass: Mutex::new(None),
                drivers,
                more_drivers: Mutex::new(None),
                application_name: application_name.into(),
                timing,
                state: Mutex::new(State::default()),
                waker: Mutex::new(None),
                secrets: Arc::new(tokio::sync::Mutex::new(())),
                results: Arc::default(),
                history: Mutex::new(None),
            }),
        }
    }

    pub fn set_history(&self, sink: Option<Arc<dyn crate::history::HistorySink>>) {
        *self.shared.history.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = sink;
    }

    pub fn has_history(&self) -> bool {
        self.shared.history.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some()
    }

    pub(crate) fn record(&self, ran: crate::history::Ran) {
        let sink = self.shared.history.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(sink) = sink {
            sink.record(ran);
        }
    }

    pub fn set_waker(&self, waker: Waker) {
        *self.shared.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(waker);
    }

    pub fn set_more_drivers(&self, more: Option<MoreDrivers>) {
        *self.shared.more_drivers.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = more;
    }

    fn more_drivers(&self) -> Vec<Arc<dyn Driver>> {
        let more = self.shared.more_drivers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let more = more.map(|more| more()).unwrap_or_default();
        more.into_iter().filter(|driver| !self.shared.drivers.iter().any(|own| own.id() == driver.id())).collect()
    }

    pub fn drivers(&self) -> Vec<Arc<dyn Driver>> {
        let mut drivers = self.shared.drivers.clone();
        drivers.extend(self.more_drivers());
        drivers
    }

    pub fn driver(&self, id: &str) -> Option<Arc<dyn Driver>> {
        self.shared
            .drivers
            .iter()
            .find(|driver| driver.id() == id)
            .cloned()
            .or_else(|| self.more_drivers().into_iter().find(|driver| driver.id() == id))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wake(&self) {
        let waker = self.shared.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    fn begin(
        &self,
        kind: OperationKind,
        connection: Option<&str>,
        context: Option<ContextKind>,
    ) -> (u64, CancellationToken) {
        let mut state = self.state();
        state.next += 1;
        let id = state.next;
        let token = CancellationToken::new();
        let op = OperationState {
            id,
            kind,
            connection: connection.map(str::to_owned),
            context,
            status: OperationStatus::Running,
            elapsed_ms: 0,
            error: None,
            execution: None,
            nodes: None,
            test: None,
            key: None,
            introspected: None,
            reconnected: false,
            saved: None,
            sessions: None,
            ended: None,
        };
        state.operations.insert(id, Op { state: op, started: Instant::now(), token: token.clone() });
        while state.operations.len() > KEPT_OPERATIONS {
            let Some(oldest) =
                state.operations.iter().find(|(_, op)| op.state.status != OperationStatus::Running).map(|(id, _)| *id)
            else {
                break;
            };
            state.operations.remove(&oldest);
            state.snapshots.remove(&oldest);
            state.plans.remove(&oldest);
        }
        (id, token)
    }

    fn finish(&self, id: u64, result: Result<Outcome, DbError>) {
        {
            let mut guard = self.state();
            let state = &mut *guard;
            if let Some(op) = state.operations.get_mut(&id) {
                op.state.elapsed_ms = u64::try_from(op.started.elapsed().as_millis()).unwrap_or(u64::MAX);
                match result {
                    Ok(outcome) => {
                        op.state.status = OperationStatus::Done;
                        match outcome {
                            Outcome::Nothing => {}
                            Outcome::Execution(execution) => op.state.execution = Some(execution),
                            Outcome::Nodes(nodes) => op.state.nodes = Some(nodes),
                            Outcome::Test(report) => op.state.test = Some(report),
                            Outcome::Key(key) => op.state.key = Some(*key),
                            Outcome::Sessions(listed) => op.state.sessions = Some(*listed),
                            Outcome::Ended(ended) => op.state.ended = Some(ended),
                            Outcome::Snapshot(snapshot, introspected) => {
                                op.state.introspected = Some(introspected);
                                state.snapshots.insert(id, *snapshot);
                            }
                            Outcome::Plan(plan) => {
                                state.plans.insert(id, *plan);
                            }
                        }
                    }
                    Err(error) => {
                        op.state.status = if error.kind == ErrorKind::Cancelled {
                            OperationStatus::Cancelled
                        } else {
                            OperationStatus::Failed
                        };
                        op.state.error = Some(error);
                    }
                }
            }
        }
        self.wake();
    }

    fn spawn<F>(&self, id: u64, work: F)
    where
        F: Future<Output = Result<Outcome, DbError>> + Send + 'static,
    {
        let engine = self.clone();
        runtime().spawn(async move {
            let result = work.await;
            engine.finish(id, result);
        });
        self.monitor();
    }

    fn mark_reconnected(&self, id: u64) {
        if let Some(op) = self.state().operations.get_mut(&id) {
            op.state.reconnected = true;
        }
    }

    pub fn operation(&self, id: u64) -> Option<OperationState> {
        let state = self.state();
        let op = state.operations.get(&id)?;
        let mut answer = op.state.clone();
        if answer.status == OperationStatus::Running {
            answer.elapsed_ms = u64::try_from(op.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        Some(answer)
    }

    pub fn wait(&self, id: u64, timeout: Duration) -> Option<OperationState> {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.operation(id)?;
            if state.status != OperationStatus::Running || Instant::now() >= deadline {
                return Some(state);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn cancel(&self, id: u64) -> bool {
        let state = self.state();
        match state.operations.get(&id) {
            Some(op) if op.state.status == OperationStatus::Running => {
                op.token.cancel();
                true
            }
            _ => false,
        }
    }

    pub fn cancel_connection(&self, connection: &str, context: Option<&ContextKind>) -> Vec<u64> {
        let state = self.state();
        state
            .operations
            .values()
            .filter(|op| {
                op.state.status == OperationStatus::Running
                    && op.state.connection.as_deref() == Some(connection)
                    && context.is_none_or(|context| op.state.context.as_ref() == Some(context))
                    && op.state.kind != OperationKind::Disconnect
            })
            .map(|op| {
                op.token.cancel();
                op.state.id
            })
            .collect()
    }

    pub fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.state().events)
    }

    fn push_event(&self, event: Event) {
        self.state().events.push(event);
        self.wake();
    }

    pub fn password(&self, id: &str) -> Option<Secret> {
        self.state().connections.get(id).and_then(|conn| conn.request.password.clone())
    }

    pub fn status(&self, id: &str) -> ConnectionStatus {
        let state = self.state();
        let Some(conn) = state.connections.get(id) else {
            return ConnectionStatus::disconnected(id);
        };
        let contexts = conn
            .info
            .iter()
            .map(|(context, info)| ContextStatus {
                context: context.clone(),
                open: info.open,
                in_transaction: info.in_transaction,
                busy: info.busy > 0,
                idle_seconds: info.last_used.map_or(0, |used| used.elapsed().as_secs()),
            })
            .collect();
        ConnectionStatus {
            connection: id.to_owned(),
            state: conn.state,
            closed_because: conn.closed_because,
            error: conn.error.clone(),
            operation: conn.connect_op,
            waiting_for: conn.waiting.filter(|_| conn.state == ConnectionState::Connecting),
            connected_seconds: conn.connected_at.map(|at| at.elapsed().as_secs()),
            auto_commit: conn.request.tracked_with(&ContextKind::Main, &state.modes).auto_commit
                && !conn.info.get(&ContextKind::Main).is_some_and(|info| info.smart_manual),
            pooler: conn.request.source.flag(POOLER),
            detected_profile: conn.detected.clone(),
            contexts,
        }
    }

    pub fn known(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.state().connections.keys().cloned().collect();
        ids.sort();
        ids
    }

    fn spec(&self, request: &ConnectRequest, context: &ContextKind, tracked: TrackedState) -> ConnectSpec {
        let timing = &self.shared.timing;
        let metadata = *context == ContextKind::Metadata;
        ConnectSpec {
            source: request.source.clone(),
            password: request.password.clone(),
            context: context.clone(),
            guards: Guards {
                application_name: self.shared.application_name.clone(),
                statement_timeout: request.statement_timeout(),
                lock_timeout: metadata.then_some(timing.metadata_lock_timeout),
                idle_in_transaction_timeout: metadata.then_some(timing.metadata_idle_transaction),
            },
            state: tracked,
            timeout: timing.connect_timeout,
            via: None,
        }
    }

    fn check_password(driver: &dyn Driver, request: &ConnectRequest) -> Result<(), DbError> {
        let wants = driver.capabilities().password && request.source.auth == crate::registry::AuthKind::Password;
        if wants && request.password.is_none() && request.stored.is_none() {
            return Err(DbError::new(ErrorKind::PasswordNeeded, ""));
        }
        Ok(())
    }

    pub fn connect(&self, request: ConnectRequest) -> Result<u64, DbError> {
        let driver = self
            .driver(&request.source.driver)
            .ok_or_else(|| DbError::new(ErrorKind::Unsupported, &request.source.driver))?;
        Self::check_password(driver.as_ref(), &request)?;
        let id = request.source.id.clone();
        {
            let state = self.state();
            if let Some(conn) = state.connections.get(&id) {
                match conn.state {
                    ConnectionState::Connecting => {
                        return conn.connect_op.ok_or_else(|| DbError::new(ErrorKind::Invalid, ""));
                    }
                    ConnectionState::Connected => {
                        let (op, _) = {
                            drop(state);
                            self.begin(OperationKind::Connect, Some(&id), None)
                        };
                        self.finish(op, Ok(Outcome::Nothing));
                        return Ok(op);
                    }
                    _ => {}
                }
            }
        }
        let (op, token) = self.begin(OperationKind::Connect, Some(&id), None);
        {
            let mut state = self.state();
            let contexts = Self::context_cells(&request, driver.as_ref(), &state.modes);
            state.connections.insert(
                id.clone(),
                Conn {
                    tunnel: Arc::default(),
                    request: request.clone(),
                    driver: Arc::clone(&driver),
                    state: ConnectionState::Connecting,
                    closed_because: None,
                    error: None,
                    connect_op: Some(op),
                    waiting: None,
                    connected_at: None,
                    info: contexts.keys().map(|context| (context.clone(), CtxInfo::default())).collect(),
                    contexts,
                    last_used: Instant::now(),
                    detected: None,
                },
            );
        }
        let engine = self.clone();
        let stored = Self::stored(driver.as_ref(), &request);
        self.spawn(op, async move {
            let opened = tokio::select! {
                opened = engine.read_password_and_open(&id, stored) => opened,
                () = token.cancelled() => Err(DbError::new(ErrorKind::Cancelled, "")),
            };
            engine.connected(&id, opened.clone());
            opened.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    fn context_cells(
        request: &ConnectRequest,
        driver: &dyn Driver,
        modes: &HashMap<(String, ContextKind), transactions::ModeSetting>,
    ) -> BTreeMap<ContextKind, CtxCell> {
        let shared = driver.capabilities().shared_connection;
        let kinds: &[ContextKind] =
            if shared { &[ContextKind::Main] } else { &[ContextKind::Main, ContextKind::Metadata] };
        kinds
            .iter()
            .map(|kind| {
                let ctx = Ctx {
                    session: None,
                    tracked: request.tracked_with(kind, modes),
                    last_used: Instant::now(),
                    opened_once: false,
                };
                (kind.clone(), Arc::new(tokio::sync::Mutex::new(ctx)))
            })
            .collect()
    }

    pub fn secret_work(
        &self,
        kind: OperationKind,
        connection: &str,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> u64 {
        let (op, _) = self.begin(kind, Some(connection), None);
        let engine = self.clone();
        let order = Arc::clone(&self.shared.secrets);
        let connection = connection.to_owned();
        self.spawn(op, async move {
            let _order = order.lock_owned().await;
            let done = tokio::task::spawn_blocking(work)
                .await
                .map_err(|error| DbError::new(ErrorKind::Invalid, error.to_string()))?;
            done.map(|()| Outcome::Nothing).map_err(|detail| {
                let stored = kind == OperationKind::StoreSecret;
                engine.push_event(Event::SecretFailed { connection, stored, detail: detail.clone() });
                DbError::new(ErrorKind::Secret, detail)
            })
        });
        op
    }

    fn stored(driver: &dyn Driver, request: &ConnectRequest) -> Option<StoredSecret> {
        let wants = driver.capabilities().password && request.source.auth == crate::registry::AuthKind::Password;
        if wants && request.password.is_none() { request.stored.clone() } else { None }
    }

    async fn read_secret(&self, stored: StoredSecret) -> Result<Secret, DbError> {
        stored.read(Arc::clone(&self.shared.secrets), self.shared.timing.secret_timeout).await
    }

    fn waiting(&self, id: &str, waiting: Waiting) {
        if let Some(conn) = self.state().connections.get_mut(id) {
            conn.waiting = Some(waiting);
        }
    }

    async fn read_password_and_open(&self, id: &str, stored: Option<StoredSecret>) -> Result<(), DbError> {
        if let Some(stored) = stored {
            self.waiting(id, Waiting::Keychain);
            let password = self.read_secret(stored).await?;
            if let Some(conn) = self.state().connections.get_mut(id) {
                conn.request.password = Some(password);
            }
        }
        self.open_all(id).await
    }

    async fn open_all(&self, id: &str) -> Result<(), DbError> {
        let cells: Vec<(ContextKind, CtxCell)> = {
            let state = self.state();
            let conn = state.connections.get(id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
            conn.contexts.iter().map(|(kind, cell)| (kind.clone(), Arc::clone(cell))).collect()
        };
        for (kind, cell) in cells {
            let mut ctx = cell.lock_owned().await;
            self.open(id, &kind, &mut ctx, false).await?;
        }
        Ok(())
    }

    fn connected(&self, id: &str, opened: Result<(), DbError>) {
        {
            let mut state = self.state();
            if let Some(conn) = state.connections.get_mut(id) {
                conn.connect_op = None;
                conn.waiting = None;
                match opened {
                    Ok(()) => {
                        conn.state = ConnectionState::Connected;
                        conn.connected_at = Some(Instant::now());
                        conn.error = None;
                        conn.last_used = Instant::now();
                    }
                    Err(error) => {
                        self.close_tunnel(Arc::clone(&conn.tunnel));
                        conn.state = ConnectionState::Failed;
                        conn.error = Some(error);
                        conn.contexts.clear();
                        conn.info.clear();
                        conn.request.password = None;
                    }
                }
            }
        }
        self.wake();
    }

    async fn open(&self, id: &str, kind: &ContextKind, ctx: &mut Ctx, retry: bool) -> Result<(), DbError> {
        let (request, driver) = {
            let mut state = self.state();
            let conn = state.connections.get_mut(id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
            if conn.info.get_mut(kind).is_some_and(|info| std::mem::take(&mut info.smart_manual)) {
                ctx.tracked.auto_commit = true;
            }
            (conn.request.clone(), Arc::clone(&conn.driver))
        };
        if (request.source.read_only || request.kind.read_only)
            && let Some(writes) =
                request.source.bootstrap.init_statements.iter().find(|init| !driver.assess(init).allowed_read_only())
        {
            let verb = crate::sql::first_keyword(writes).unwrap_or_default();
            return Err(DbError::new(ErrorKind::Refused, verb).with_code("readOnly"));
        }
        let mut spec = self.spec(&request, kind, ctx.tracked.clone());
        spec.via = self.tunnel_address(id).await?;
        self.waiting(id, Waiting::Network);
        let attempts: Vec<Duration> = if retry { self.shared.timing.backoff.clone() } else { vec![Duration::ZERO] };
        let mut last = DbError::new(ErrorKind::Connection, "");
        for wait in attempts {
            tokio::time::sleep(wait).await;
            match tokio::time::timeout(spec.timeout, driver.connect(&spec)).await {
                Ok(Ok(session)) => {
                    ctx.session = Some(session);
                    ctx.last_used = Instant::now();
                    let again = std::mem::replace(&mut ctx.opened_once, true);
                    self.update_info(id, kind, ctx);
                    if again {
                        self.push_event(Event::Reconnected { connection: id.to_owned(), context: kind.clone() });
                    }
                    return Ok(());
                }
                Ok(Err(error)) => {
                    let lasting = !matches!(error.kind, ErrorKind::Connection | ErrorKind::Timeout);
                    last = error;
                    if lasting {
                        break;
                    }
                }
                Err(_) => last = DbError::new(ErrorKind::Timeout, ""),
            }
        }
        Err(last)
    }

    fn detected(&self, id: &str, stored: Option<&str>, detected: Option<String>) {
        let detected = detected.filter(|found| Some(found.as_str()) != stored);
        let event = {
            let mut state = self.state();
            let Some(conn) = state.connections.get_mut(id) else { return };
            if conn.detected == detected {
                return;
            }
            conn.detected.clone_from(&detected);
            detected.map(|profile| Event::ProfileDetected {
                connection: id.to_owned(),
                name: conn.request.source.name.clone(),
                profile,
            })
        };
        if let Some(event) = event {
            self.push_event(event);
        }
    }

    fn update_info(&self, id: &str, kind: &ContextKind, ctx: &Ctx) {
        let mut state = self.state();
        if let Some(info) = state.connections.get_mut(id).and_then(|conn| conn.info.get_mut(kind)) {
            info.open = ctx.session.is_some();
            info.server_id = ctx.session.as_ref().and_then(|session| session.server_id());
            info.in_transaction = ctx.session.as_ref().is_some_and(|session| session.in_transaction());
            info.last_used = Some(ctx.last_used);
            if info.in_transaction {
                if let Some((_, token)) = info.ending.take() {
                    token.cancel();
                }
            } else {
                info.end_transaction();
            }
        }
    }

    fn busy(&self, id: &str, kind: &ContextKind, delta: isize) {
        let mut state = self.state();
        if let Some(conn) = state.connections.get_mut(id) {
            if delta > 0 {
                conn.last_used = Instant::now();
            }
            let info = conn.info.entry(kind.clone()).or_default();
            info.busy = info.busy.saturating_add_signed(delta);
        }
    }

    fn cell(&self, id: &str, kind: &ContextKind) -> Result<(ContextKind, CtxCell), DbError> {
        let mut guard = self.state();
        let State { connections, modes, .. } = &mut *guard;
        let conn = connections.get_mut(id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        match conn.state {
            ConnectionState::Connected => {}
            ConnectionState::Closed => {
                conn.state = ConnectionState::Connected;
                conn.closed_because = None;
                conn.connected_at = Some(Instant::now());
            }
            ConnectionState::Failed => {
                return Err(conn.error.clone().unwrap_or_else(|| DbError::new(ErrorKind::NotConnected, "")));
            }
            _ => return Err(DbError::new(ErrorKind::NotConnected, "")),
        }
        let kind = if conn.driver.capabilities().shared_connection { ContextKind::Main } else { kind.clone() };
        let request = conn.request.clone();
        let cell = conn.contexts.entry(kind.clone()).or_insert_with(|| {
            let ctx = Ctx {
                session: None,
                tracked: request.tracked_with(&kind, modes),
                last_used: Instant::now(),
                opened_once: false,
            };
            Arc::new(tokio::sync::Mutex::new(ctx))
        });
        let cell = Arc::clone(cell);
        conn.info.entry(kind.clone()).or_default();
        Ok((kind, cell))
    }

    async fn ready(&self, id: &str, kind: &ContextKind, ctx: &mut Ctx, op: u64) -> Result<(), DbError> {
        if ctx.session.as_ref().is_some_and(|session| session.is_closed()) {
            ctx.session = None;
        }
        if let Some(session) = ctx.session.as_mut()
            && ctx.last_used.elapsed() >= self.shared.timing.ping_after
        {
            let answered = tokio::time::timeout(self.shared.timing.ping_timeout, session.ping()).await;
            if !matches!(answered, Ok(Ok(()))) {
                ctx.session = None;
            }
        }
        if ctx.session.is_none() {
            let was_open = ctx.opened_once;
            self.open(id, kind, ctx, true).await?;
            if was_open {
                self.mark_reconnected(op);
            }
        }
        Ok(())
    }

    pub fn execute(
        &self,
        id: &str,
        context: ContextKind,
        statement: &str,
        options: ExecOptions,
    ) -> Result<u64, DbError> {
        let (kind, cell) = self.cell(id, &context)?;
        let (op, token) = self.begin(OperationKind::Execute, Some(id), Some(kind.clone()));
        let (engine, id, statement) = (self.clone(), id.to_owned(), statement.to_owned());
        self.busy(&id, &kind, 1);
        self.spawn(op, async move {
            let result = engine.run_statement(&id, (&kind, cell), &statement, &options, (op, &token)).await;
            engine.busy(&id, &kind, -1);
            let driver = engine.state().connections.get(&id).map(|conn| Arc::clone(&conn.driver));
            if result.is_ok() && driver.is_some_and(|driver| driver.changes_structure(&statement)) {
                engine.push_event(Event::StructureChanged { connection: id.clone() });
            }
            result.map(Outcome::Execution)
        });
        Ok(op)
    }

    async fn run_statement(
        &self,
        id: &str,
        (kind, cell): (&ContextKind, CtxCell),
        statement: &str,
        options: &ExecOptions,
        (op, token): (u64, &CancellationToken),
    ) -> Result<Execution, DbError> {
        let mut ctx: OwnedMutexGuard<Ctx> = tokio::select! {
            ctx = cell.lock_owned() => ctx,
            () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
        };
        let driver = self.state().connections.get(id).map(|conn| Arc::clone(&conn.driver));
        let driver = driver.ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        let dialect = crate::sql::Dialect::of_language(&driver.capabilities().language);
        let read = driver.assess(statement).effect == crate::safety::Effect::Read
            && !crate::safety::calls_functions(statement, dialect);
        let mut retried = false;
        loop {
            self.ready(id, kind, &mut ctx, op).await?;
            let switched = self.smart_before(id, kind, &mut ctx, statement).await;
            let autonomous =
                ctx.tracked.auto_commit && ctx.session.as_ref().is_some_and(|session| !session.in_transaction());
            let result = self.cancellable(&mut ctx, statement, options, (token, !read)).await;
            ctx.last_used = Instant::now();
            let affected = result.as_ref().ok().and_then(|execution| execution.affected);
            self.after_statement(id, kind, &mut ctx, statement, (switched, result.is_err(), affected)).await;
            let lost = match &result {
                Err(error) => {
                    error.kind == ErrorKind::Connection
                        || ctx.session.as_ref().is_none_or(|session| session.is_closed())
                }
                Ok(_) => false,
            };
            if !lost || matches!(&result, Err(error) if matches!(error.kind, ErrorKind::Cancelled | ErrorKind::Timeout))
            {
                self.update_info(id, kind, &ctx);
                return result;
            }
            ctx.session = None;
            self.update_info(id, kind, &ctx);
            if read && autonomous && !retried {
                retried = true;
                continue;
            }
            let reopened = self.ready(id, kind, &mut ctx, op).await;
            self.update_info(id, kind, &ctx);
            let mut error = result.err().unwrap_or_else(|| DbError::new(ErrorKind::Connection, ""));
            if reopened.is_ok() && !read {
                error.code = Some("notReplayed".to_owned());
            }
            return Err(error);
        }
    }

    async fn cancellable(
        &self,
        ctx: &mut Ctx,
        statement: &str,
        options: &ExecOptions,
        (token, changes): (&CancellationToken, bool),
    ) -> Result<Execution, DbError> {
        let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        let canceller: Option<Arc<dyn Canceller>> = session.canceller();
        let wait = self.shared.timing.cancel_wait;
        let mut close = false;
        let result = {
            let run = session.execute(statement, options);
            tokio::pin!(run);
            let deadline = options.timeout;
            let expired = async move {
                match deadline {
                    Some(deadline) => tokio::time::sleep(deadline).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                result = &mut run => result,
                reason = async {
                    tokio::select! {
                        () = token.cancelled() => ErrorKind::Cancelled,
                        () = expired => ErrorKind::Timeout,
                    }
                } => {
                    match stop(run.as_mut(), canceller.as_ref(), wait).await {
                        Stopping::Ended(Ok(execution)) if changes => Ok(execution),
                        Stopping::Ended(_) => Err(DbError::new(reason, "")),
                        Stopping::Unconfirmed(code) => {
                            close = true;
                            Err(DbError::new(reason, "").with_code(code))
                        }
                    }
                }
            }
        };
        if close {
            ctx.session = None;
        }
        result.map(|mut execution| {
            if let Some(session) = ctx.session.as_mut() {
                execution.notices.extend(session.take_notices());
            }
            execution
        })
    }

    pub fn children(&self, id: &str, path: Vec<String>) -> Result<u64, DbError> {
        let (kind, cell) = self.cell(id, &ContextKind::Metadata)?;
        let (op, token) = self.begin(OperationKind::Children, Some(id), Some(kind.clone()));
        let (engine, id) = (self.clone(), id.to_owned());
        self.busy(&id, &kind, 1);
        self.spawn(op, async move {
            let result = async {
                let mut ctx = cell.lock_owned().await;
                engine.ready(&id, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                let (canceller, wait) = (session.canceller(), engine.shared.timing.cancel_wait);
                let (nodes, close) = stop_or_close(session.children(&path), canceller, wait, &token).await;
                if close {
                    ctx.session = None;
                }
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                nodes
            }
            .await;
            engine.busy(&id, &kind, -1);
            result.map(Outcome::Nodes)
        });
        Ok(op)
    }

    pub fn key(&self, id: &str, path: Vec<String>, request: crate::driver::KeyRequest) -> Result<u64, DbError> {
        if !self.state().connections.contains_key(id) {
            return Err(DbError::new(ErrorKind::NotConnected, ""));
        }
        let (op, token) = self.begin(OperationKind::Key, Some(id), Some(ContextKind::Main));
        let (engine, id) = (self.clone(), id.to_owned());
        self.spawn(op, async move {
            let (kind, cell) = engine.connected_cell(&id, &ContextKind::Main, &token).await?;
            engine.busy(&id, &kind, 1);
            let result = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&id, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                let read = tokio::select! {
                    read = session.key(&path, &request) => read,
                    () = token.cancelled() => Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                read
            }
            .await;
            engine.busy(&id, &kind, -1);
            result.map(|key| Outcome::Key(Box::new(key)))
        });
        Ok(op)
    }

    pub fn introspect(
        &self,
        id: &str,
        request: IntrospectRequest,
        previous: Snapshot,
        file: Option<std::path::PathBuf>,
    ) -> Result<u64, DbError> {
        let (kind, cell) = self.cell(id, &ContextKind::Metadata)?;
        let named = self.state().connections.get(id).map(|conn| {
            (conn.driver.id().to_owned(), conn.request.source.profile.clone(), conn.request.source.host.clone())
        });
        let (op, token) = self.begin(OperationKind::Introspect, Some(id), Some(kind.clone()));
        let (engine, id) = (self.clone(), id.to_owned());
        self.busy(&id, &kind, 1);
        self.spawn(op, async move {
            let result = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&id, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                let version = session.server_version().await.ok().flatten();
                let (canceller, wait) = (session.canceller(), engine.shared.timing.cancel_wait);
                let (read, close) =
                    stop_or_close(session.introspect(&request, Some(&previous)), canceller, wait, &token).await;
                if close {
                    ctx.session = None;
                }
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                drop(ctx);
                let read = read?;
                let mut snapshot = previous;
                snapshot.catalogs = read.catalogs;
                if let Some((driver, stored, host)) = &named {
                    let detected = crate::profile::detect(driver, version.as_deref().unwrap_or(""), host.as_deref())
                        .map(|profile| profile.id.to_owned());
                    snapshot.profile = stored.clone().or_else(|| detected.clone());
                    engine.detected(&id, stored.as_deref(), detected);
                }
                snapshot.server_version = version.or(snapshot.server_version);
                snapshot.taken =
                    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
                let saved = match file {
                    Some(file) => {
                        let written = snapshot.clone();
                        tokio::task::spawn_blocking(move || crate::schema::write_snapshot(&file, &written))
                            .await
                            .is_ok_and(|written| written.is_ok())
                    }
                    None => false,
                };
                let introspected = Introspected {
                    read: read.read,
                    unchanged: read.unchanged,
                    objects: snapshot.object_count(),
                    saved,
                };
                Ok(Outcome::Snapshot(Box::new(snapshot), introspected))
            }
            .await;
            engine.busy(&id, &kind, -1);
            result
        });
        Ok(op)
    }

    pub fn take_snapshot(&self, op: u64) -> Option<Snapshot> {
        self.state().snapshots.remove(&op)
    }

    pub fn read_only(&self, id: &str) -> bool {
        self.state()
            .connections
            .get(id)
            .is_some_and(|conn| conn.request.source.read_only || conn.request.kind.read_only)
    }

    pub fn set_read_only(&self, id: &str, read_only: bool) -> Vec<u64> {
        let cells: Vec<(ContextKind, CtxCell)> = {
            let mut state = self.state();
            let Some(conn) = state.connections.get_mut(id) else { return Vec::new() };
            conn.request.source.read_only = read_only;
            if !matches!(conn.state, ConnectionState::Connected | ConnectionState::Closed) {
                return Vec::new();
            }
            conn.contexts
                .iter()
                .filter(|(kind, _)| **kind != ContextKind::Metadata)
                .map(|(kind, cell)| (kind.clone(), Arc::clone(cell)))
                .collect()
        };
        let effective = self.read_only(id);
        let mut operations = Vec::new();
        for (kind, cell) in cells {
            let (op, _) = self.begin(OperationKind::Apply, Some(id), Some(kind.clone()));
            let (engine, id) = (self.clone(), id.to_owned());
            self.spawn(op, async move {
                let mut ctx = cell.lock_owned().await;
                if ctx.tracked.read_only == effective {
                    return Ok(Outcome::Nothing);
                }
                let tracked = TrackedState { read_only: effective, ..ctx.tracked.clone() };
                if let Some(session) = ctx.session.as_mut() {
                    session.apply(&tracked).await?;
                }
                ctx.tracked = tracked;
                engine.update_info(&id, &kind, &ctx);
                Ok(Outcome::Nothing)
            });
            operations.push(op);
        }
        operations
    }

    pub fn set_kind(
        &self,
        id: &str,
        kind: ConnectionType,
        overrides: crate::registry::TypeOverrides,
    ) -> (Vec<u64>, Vec<ContextKind>) {
        let (cells, kept) = {
            let mut guard = self.state();
            let State { connections, modes, .. } = &mut *guard;
            let Some(conn) = connections.get_mut(id) else { return (Vec::new(), Vec::new()) };
            conn.request.source.connection_type.clone_from(&kind.id);
            conn.request.kind = kind;
            conn.request.source.overrides = overrides;
            for ((known, _), setting) in modes.iter_mut() {
                if known == id {
                    setting.mode = None;
                }
            }
            let mut kept = Vec::new();
            for (context, info) in &conn.info {
                if *context == ContextKind::Metadata || !info.in_transaction || info.modifying == 0 {
                    continue;
                }
                if conn.request.tracked(context).auto_commit {
                    let setting = modes.entry((id.to_owned(), context.clone())).or_default();
                    setting.mode = Some(CommitMode::Manual);
                    kept.push(context.clone());
                }
            }
            if !matches!(conn.state, ConnectionState::Connected | ConnectionState::Closed) {
                return (Vec::new(), kept);
            }
            let cells: Vec<(ContextKind, CtxCell)> = conn
                .contexts
                .iter()
                .filter(|(context, _)| **context != ContextKind::Metadata)
                .map(|(context, cell)| (context.clone(), Arc::clone(cell)))
                .collect();
            (cells, kept)
        };
        let mut operations = Vec::new();
        for (context, cell) in cells {
            let (op, _) = self.begin(OperationKind::Apply, Some(id), Some(context.clone()));
            let (engine, id) = (self.clone(), id.to_owned());
            self.spawn(op, async move {
                let mut ctx = cell.lock_owned().await;
                let tracked = {
                    let state = engine.state();
                    let conn = state.connections.get(&id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                    TrackedState {
                        database: ctx.tracked.database.clone(),
                        schema: ctx.tracked.schema.clone(),
                        isolation: ctx.tracked.isolation.clone(),
                        ..conn.request.tracked_with(&context, &state.modes)
                    }
                };
                if ctx.tracked == tracked {
                    return Ok(Outcome::Nothing);
                }
                if let Some(session) = ctx.session.as_mut() {
                    if session.in_transaction() && tracked.auto_commit {
                        session.rollback().await?;
                    }
                    session.apply(&tracked).await?;
                }
                ctx.tracked = tracked;
                engine.set_smart_manual(&id, &context, false);
                engine.update_info(&id, &context, &ctx);
                Ok(Outcome::Nothing)
            });
            operations.push(op);
        }
        (operations, kept)
    }

    pub fn apply(&self, id: &str, context: ContextKind, tracked: TrackedState) -> Result<u64, DbError> {
        let (kind, cell) = self.cell(id, &context)?;
        let pooler = self.state().connections.get(id).is_some_and(|conn| conn.request.source.flag(POOLER));
        let tracked = TrackedState { auto_commit: tracked.auto_commit || pooler, ..tracked };
        let (op, _) = self.begin(OperationKind::Apply, Some(id), Some(kind.clone()));
        let (engine, id) = (self.clone(), id.to_owned());
        self.busy(&id, &kind, 1);
        self.spawn(op, async move {
            let result = async {
                let mut ctx = cell.lock_owned().await;
                engine.ready(&id, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                session.apply(&tracked).await?;
                ctx.tracked = tracked;
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                Ok(Outcome::Nothing)
            }
            .await;
            engine.busy(&id, &kind, -1);
            result
        });
        Ok(op)
    }

    pub fn test(&self, request: ConnectRequest) -> Result<u64, DbError> {
        let driver = self
            .driver(&request.source.driver)
            .ok_or_else(|| DbError::new(ErrorKind::Unsupported, &request.source.driver))?;
        Self::check_password(driver.as_ref(), &request)?;
        let (op, token) = self.begin(OperationKind::Test, Some(&request.source.id), None);
        let mut spec = self.spec(&request, &ContextKind::Main, request.tracked(&ContextKind::Main));
        let stored = Self::stored(driver.as_ref(), &request);
        let engine = self.clone();
        let tunnelled: TunnelSlot = Arc::default();
        let kept = Arc::clone(&tunnelled);
        self.spawn(op, async move {
            let work = async {
                if let Some(stored) = stored {
                    spec.password = Some(engine.read_secret(stored).await?);
                }
                if let Some((tunnel, token)) = engine.open_tunnel(&request).await? {
                    spec.via = Some(tunnel.address());
                    *kept.lock().await = Some((tunnel, token));
                }
                let started = Instant::now();
                let mut session = tokio::time::timeout(spec.timeout, driver.connect(&spec))
                    .await
                    .map_err(|_| DbError::new(ErrorKind::Timeout, ""))??;
                let connect_ms = millis(started.elapsed());
                let pinged = Instant::now();
                session.ping().await?;
                let round_trip_ms = millis(pinged.elapsed());
                let server_version = session.server_version().await.ok().flatten();
                let profile = crate::profile::detect(
                    driver.id(),
                    server_version.as_deref().unwrap_or(""),
                    spec.source.host.as_deref(),
                )
                .map(|profile| profile.id.to_owned());
                Ok(Outcome::Test(TestReport { server_version, connect_ms, round_trip_ms, profile }))
            };
            let result = tokio::select! {
                result = work => result,
                () = token.cancelled() => Err(DbError::new(ErrorKind::Cancelled, "")),
            };
            engine.close_tunnel(tunnelled);
            result
        });
        Ok(op)
    }

    pub fn open_transactions(&self, id: &str) -> Vec<ContextKind> {
        let state = self.state();
        state
            .connections
            .get(id)
            .map(|conn| {
                conn.info.iter().filter(|(_, info)| info.in_transaction).map(|(kind, _)| kind.clone()).collect()
            })
            .unwrap_or_default()
    }

    pub fn disconnect(&self, id: &str, policy: TransactionPolicy) -> Result<u64, DbError> {
        let open = self.changes_pending(id);
        if policy == TransactionPolicy::Ask && !open.is_empty() {
            let names: Vec<String> = open.iter().map(ToString::to_string).collect();
            return Err(DbError::new(ErrorKind::OpenTransactions, names.join(", ")));
        }
        self.cancel_connection(id, None);
        let (op, _) = self.begin(OperationKind::Disconnect, Some(id), None);
        let (engine, id) = (self.clone(), id.to_owned());
        self.spawn(op, async move {
            let cells = engine.take_cells(&id, ConnectionState::Disconnected, None);
            let result = close_cells(cells, policy).await;
            engine.close_tunnel_of(&id);
            engine.wake();
            result.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    pub fn disconnect_all(&self, policy: TransactionPolicy) -> Result<Vec<u64>, DbError> {
        let ids: Vec<String> = {
            let state = self.state();
            state
                .connections
                .iter()
                .filter(|(_, conn)| conn.state != ConnectionState::Disconnected)
                .map(|(id, _)| id.clone())
                .collect()
        };
        if policy == TransactionPolicy::Ask {
            let open: Vec<String> = ids
                .iter()
                .flat_map(|id| self.changes_pending(id).into_iter().map(move |kind| format!("{id} {kind}")))
                .collect();
            if !open.is_empty() {
                return Err(DbError::new(ErrorKind::OpenTransactions, open.join(", ")));
            }
        }
        ids.iter().map(|id| self.disconnect(id, policy)).collect()
    }

    pub fn invalidate(&self, id: &str, policy: TransactionPolicy) -> Result<u64, DbError> {
        {
            let state = self.state();
            let conn = state.connections.get(id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
            if !matches!(conn.state, ConnectionState::Connected | ConnectionState::Closed) {
                return Err(DbError::new(ErrorKind::NotConnected, ""));
            }
        }
        let open = self.changes_pending(id);
        if policy == TransactionPolicy::Ask && !open.is_empty() {
            let names: Vec<String> = open.iter().map(ToString::to_string).collect();
            return Err(DbError::new(ErrorKind::OpenTransactions, names.join(", ")));
        }
        self.cancel_connection(id, None);
        let (op, _) = self.begin(OperationKind::Invalidate, Some(id), None);
        let (engine, id) = (self.clone(), id.to_owned());
        self.spawn(op, async move {
            let cells: Vec<(ContextKind, CtxCell)> = {
                let state = engine.state();
                let conn = state.connections.get(&id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                conn.contexts.iter().map(|(kind, cell)| (kind.clone(), Arc::clone(cell))).collect()
            };
            for (kind, cell) in cells {
                let mut ctx = cell.lock_owned().await;
                if policy == TransactionPolicy::Commit
                    && let Some(session) = ctx.session.as_mut()
                    && session.in_transaction()
                {
                    session.commit().await?;
                }
                ctx.session = None;
                engine.update_info(&id, &kind, &ctx);
                engine.open(&id, &kind, &mut ctx, true).await?;
            }
            engine.mark_reconnected(op);
            Ok(Outcome::Nothing)
        });
        Ok(op)
    }

    fn take_cells(&self, id: &str, to: ConnectionState, because: Option<CloseReason>) -> Vec<(ContextKind, CtxCell)> {
        let mut state = self.state();
        let Some(conn) = state.connections.get_mut(id) else { return Vec::new() };
        conn.state = to;
        conn.closed_because = because;
        conn.connected_at = None;
        if to == ConnectionState::Disconnected {
            conn.request.password = None;
            conn.info.clear();
            std::mem::take(&mut conn.contexts).into_iter().collect()
        } else {
            for info in conn.info.values_mut() {
                info.open = false;
                info.end_transaction();
            }
            conn.contexts.iter().map(|(kind, cell)| (kind.clone(), Arc::clone(cell))).collect()
        }
    }

    pub fn suspend_all(&self, because: CloseReason) -> usize {
        let ids: Vec<String> = {
            let state = self.state();
            state
                .connections
                .iter()
                .filter(|(_, conn)| conn.state == ConnectionState::Connected)
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &ids {
            self.close_by_itself(id, because);
        }
        ids.len()
    }

    fn close_by_itself(&self, id: &str, because: CloseReason) {
        let cells = self.take_cells(id, ConnectionState::Closed, Some(because));
        let (engine, id) = (self.clone(), id.to_owned());
        runtime().spawn(async move {
            for (_, cell) in cells {
                let mut ctx = cell.lock_owned().await;
                ctx.session = None;
            }
            engine.close_tunnel_of(&id);
        });
        self.wake();
    }

    fn monitor(&self) {
        {
            let mut state = self.state();
            if std::mem::replace(&mut state.monitoring, true) {
                return;
            }
        }
        let weak = Arc::downgrade(&self.shared);
        let tick = self.shared.timing.tick;
        runtime().spawn(async move {
            let mut wall = SystemTime::now();
            let mut mono = Instant::now();
            loop {
                tokio::time::sleep(tick).await;
                let Some(shared) = weak.upgrade() else { return };
                let engine = Engine { shared };
                let (now_wall, now_mono) = (SystemTime::now(), Instant::now());
                let walked = now_wall.duration_since(wall).unwrap_or_default();
                let counted = now_mono.duration_since(mono);
                (wall, mono) = (now_wall, now_mono);
                if walked > counted + engine.shared.timing.sleep_gap {
                    let count = engine.suspend_all(CloseReason::Sleep);
                    if count > 0 {
                        engine.push_event(Event::Slept { count });
                    }
                    continue;
                }
                engine.check_idle().await;
            }
        });
    }

    pub async fn check_idle(&self) {
        let mut closing = Vec::new();
        let mut transactions = Vec::new();
        {
            let state = self.state();
            for (id, conn) in &state.connections {
                if conn.state != ConnectionState::Connected || conn.info.values().any(|info| info.busy > 0) {
                    continue;
                }
                if let Some(limit) = conn.request.close_idle_after()
                    && conn.last_used.elapsed() >= limit
                {
                    closing.push((id.clone(), conn.request.source.name.clone()));
                    continue;
                }
                if let Some(limit) = conn.request.end_idle_transactions_after() {
                    for (kind, info) in &conn.info {
                        if info.in_transaction
                            && info.ending.is_none()
                            && info.last_used.is_some_and(|used| used.elapsed() >= limit)
                        {
                            transactions.push((id.clone(), conn.request.source.name.clone(), kind.clone()));
                        }
                    }
                }
            }
        }
        for (id, name) in closing {
            self.close_by_itself(&id, CloseReason::Idle);
            self.push_event(Event::IdleClosed { connection: id, name });
        }
        for (id, name, kind) in transactions {
            self.announce_idle(&id, &name, &kind);
        }
    }

    pub fn forget(&self, id: &str) {
        let cells = self.take_cells(id, ConnectionState::Disconnected, None);
        let tunnel = self.state().connections.remove(id).map(|conn| conn.tunnel);
        let engine = self.clone();
        runtime().spawn(async move {
            let _ = close_cells(cells, TransactionPolicy::Rollback).await;
            if let Some(tunnel) = tunnel {
                engine.close_tunnel(tunnel);
            }
        });
    }

    fn close_tunnel_of(&self, id: &str) {
        let slot = self.state().connections.get(id).map(|conn| Arc::clone(&conn.tunnel));
        if let Some(slot) = slot {
            self.close_tunnel(slot);
        }
    }
}

enum Stopping<T> {
    Ended(Result<T, DbError>),
    Unconfirmed(&'static str),
}

async fn stop<T, F: std::future::Future<Output = Result<T, DbError>>>(
    mut run: std::pin::Pin<&mut F>,
    canceller: Option<&Arc<dyn Canceller>>,
    wait: Duration,
) -> Stopping<T> {
    let started = Instant::now();
    let Some(canceller) = canceller else { return Stopping::Unconfirmed("noServerCancel") };
    let asked = tokio::select! {
        result = &mut run => return Stopping::Ended(result),
        asked = tokio::time::timeout(wait, canceller.cancel()) => matches!(asked, Ok(Ok(()))),
    };
    let left = wait.saturating_sub(started.elapsed());
    match tokio::time::timeout(left, &mut run).await {
        Ok(result) if asked => Stopping::Ended(result),
        _ => Stopping::Unconfirmed("cancelUnconfirmed"),
    }
}

async fn stop_or_close<T>(
    run: impl std::future::Future<Output = Result<T, DbError>>,
    canceller: Option<Arc<dyn Canceller>>,
    wait: Duration,
    token: &CancellationToken,
) -> (Result<T, DbError>, bool) {
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => (result, false),
        () = token.cancelled() => match stop(run.as_mut(), canceller.as_ref(), wait).await {
            Stopping::Ended(_) => (Err(DbError::new(ErrorKind::Cancelled, "")), false),
            Stopping::Unconfirmed(code) => (Err(DbError::new(ErrorKind::Cancelled, "").with_code(code)), true),
        },
    }
}

async fn close_cells(cells: Vec<(ContextKind, CtxCell)>, policy: TransactionPolicy) -> Result<(), DbError> {
    let mut failed = None;
    for (_, cell) in cells {
        let mut ctx = cell.lock_owned().await;
        if let Some(session) = ctx.session.as_mut()
            && session.in_transaction()
        {
            let ended = match policy {
                TransactionPolicy::Commit => session.commit().await,
                TransactionPolicy::Rollback | TransactionPolicy::Ask => session.rollback().await,
            };
            if let Err(error) = ended {
                failed.get_or_insert(error);
            }
        }
        ctx.session = None;
    }
    failed.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod tunnel_tests;

#[cfg(test)]
mod export_tests;

#[cfg(test)]
mod csv_import_tests;

#[cfg(test)]
mod transfer_tests;

#[cfg(test)]
mod safety_tests;

#[cfg(test)]
mod sessions_tests;
