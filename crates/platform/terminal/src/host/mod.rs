pub mod agents;
mod install;
pub mod protocol;
pub mod record;
mod server;

use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use protocol::{Frame, Message, read_frame, write_frame};
pub use protocol::{OLDEST, PROTOCOL, SessionInfo, Speaks};
pub use server::{serve, serve_with};

pub const PROGRAM: &str = "zen-pty-host";

pub const PROGRAM_VARIABLE: &str = "ZEN_PTY_HOST";

pub const DEFAULT_IDLE: Duration = Duration::from_secs(60);

const SOCKET_LIMIT: usize = 100;
const START_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    dir: PathBuf,
    program: Option<PathBuf>,
    idle: Duration,
    speaks: Speaks,
}

#[derive(Debug)]
pub(crate) struct Connection {
    pub(crate) stream: UnixStream,
    pub(crate) protocol: u32,
    pub(crate) instance: u64,
}

impl Host {
    pub fn new(dir: impl Into<PathBuf>, program: Option<PathBuf>) -> Host {
        Host { dir: dir.into(), program, idle: DEFAULT_IDLE, speaks: Speaks::CURRENT }
    }

    pub fn installed(dir: impl Into<PathBuf>) -> Option<Host> {
        let program = match std::env::var_os(PROGRAM_VARIABLE) {
            Some(program) if !program.is_empty() => PathBuf::from(program),
            _ => std::env::current_exe().ok()?.with_file_name(PROGRAM),
        };
        program.is_file().then(|| Host::new(dir, Some(program)))
    }

    pub fn with_idle(mut self, idle: Duration) -> Host {
        self.idle = idle;
        self
    }

    pub fn speaking(mut self, speaks: Speaks) -> Host {
        self.speaks = speaks;
        self
    }

    pub fn speaks(&self) -> Speaks {
        self.speaks
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn socket(&self) -> PathBuf {
        socket_path(&self.dir, self.speaks.protocol)
    }

    pub fn copy_folder(&self) -> PathBuf {
        install::bin_folder(&self.dir)
    }

    pub(crate) fn connect(&self) -> io::Result<Connection> {
        match self.reach(self.speaks.protocol) {
            Ok(connection) => return Ok(connection),
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Err(_) => {}
        }
        let Some(source) = &self.program else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no terminal host runs for this folder"));
        };
        let socket = self.socket();
        let started = install::Starting::lock(&self.dir)?;
        if let Ok(connection) = self.reach(self.speaks.protocol) {
            return Ok(connection);
        }
        let program = install::runnable(&self.dir, self.speaks.protocol, source)?;
        let mut child = Command::new(&program)
            .arg(&self.dir)
            .arg("--idle")
            .arg(self.idle.as_secs().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        std::thread::spawn(move || child.wait());
        let since = Instant::now();
        loop {
            match only_mine(&socket, Kind::Socket).and_then(|()| UnixStream::connect(&socket)) {
                Ok(stream) => {
                    let connection = hello(stream, self.speaks);
                    drop(started);
                    return connection;
                }
                Err(error) if since.elapsed() > START_TIMEOUT => return Err(error),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    pub(crate) fn reach(&self, protocol: u32) -> io::Result<Connection> {
        let socket = socket_path(&self.dir, protocol);
        if let Some(folder) = socket.parent().filter(|folder| folder.exists()) {
            only_mine(folder, Kind::Folder)?;
        }
        if std::fs::symlink_metadata(&socket).is_err() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no terminal host runs for this folder"));
        }
        only_mine(&socket, Kind::Socket)?;
        match UnixStream::connect(&socket) {
            Ok(stream) => hello(stream, self.speaks),
            Err(error) => Err(io::Error::new(io::ErrorKind::NotFound, error)),
        }
    }

    pub fn record(&self, hosted: crate::HostedSession) -> Option<record::Record> {
        record::read(&self.dir, hosted)
    }

    pub fn forget(&self, hosted: crate::HostedSession) {
        record::forget(&self.dir, hosted);
    }

    pub fn forget_all(&self) {
        record::forget_all(&self.dir);
    }

    pub fn forget_older_than(&self, age: Duration) {
        record::forget_older_than(&self.dir, age);
        agents::forget_older_than(&self.dir, age);
    }

    pub fn agents_folder(&self) -> Option<PathBuf> {
        agents::ensure(&self.dir).ok()
    }

    pub fn agent_report(&self, terminal: &str, agent: &str) -> Option<serde_json::Value> {
        agents::read(&self.dir, terminal, agent)
    }

    pub fn report_agent(&self, terminal: &str, agent: &str, report: &serde_json::Value) -> io::Result<()> {
        agents::write(&self.dir, terminal, agent, report)
    }

    pub fn forget_agents(&self, terminal: &str) {
        agents::forget(&self.dir, terminal);
    }

    pub fn runs(&self, protocol: u32) -> bool {
        install::running(&self.dir, protocol).is_some()
    }

    pub fn list(&self) -> io::Result<Vec<SessionInfo>> {
        let mut stream = self.connect()?.stream;
        write_frame(&mut stream, &Frame::Control(Message::List))?;
        match read_frame(&mut stream)? {
            Frame::Control(Message::Sessions { sessions }) => Ok(sessions),
            other => Err(unexpected(&other)),
        }
    }
}

fn hello(mut stream: UnixStream, speaks: Speaks) -> io::Result<Connection> {
    let hello = Message::Hello { protocol: speaks.protocol, oldest: speaks.oldest, instance: None };
    write_frame(&mut stream, &Frame::Control(hello))?;
    match read_frame(&mut stream)? {
        Frame::Control(Message::Hello { protocol, oldest, instance }) => {
            if speaks.meets(Speaks { protocol, oldest }).is_some() {
                Ok(Connection { stream, protocol, instance: instance.unwrap_or(0) })
            } else {
                Err(incompatible(protocol))
            }
        }
        other => Err(unexpected(&other)),
    }
}

pub(crate) fn incompatible(protocol: u32) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, Incompatible(protocol))
}

#[derive(Debug)]
pub(crate) struct Incompatible(pub(crate) u32);

impl std::fmt::Display for Incompatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the terminal host speaks protocol {}, which this IDE does not read", self.0)
    }
}

impl std::error::Error for Incompatible {}

pub(crate) fn unexpected(frame: &Frame) -> io::Error {
    match frame {
        Frame::Control(Message::Error { message }) => io::Error::other(message.clone()),
        _ => io::Error::new(io::ErrorKind::InvalidData, "unexpected answer from the terminal host"),
    }
}

pub fn socket_path(dir: &Path, protocol: u32) -> PathBuf {
    let local = dir.join(format!("host-{protocol}.sock"));
    if local.as_os_str().len() <= SOCKET_LIMIT {
        return local;
    }
    let user = rustix::process::getuid().as_raw();
    std::env::temp_dir().join(format!("zen-pty-{user}")).join(format!("{:016x}-{protocol}.sock", fnv(dir)))
}

fn fnv(dir: &Path) -> u64 {
    dir.as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3))
}

pub(crate) fn private_folder(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if meta.uid() == rustix::process::getuid().as_raw() && meta.file_type().is_dir() {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    only_mine(dir, Kind::Folder)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Folder,
    Socket,
    File,
}

pub(crate) fn only_mine(path: &Path, kind: Kind) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let is_kind = match kind {
        Kind::Folder => meta.file_type().is_dir(),
        Kind::Socket => meta.file_type().is_socket(),
        Kind::File => meta.file_type().is_file(),
    };
    if private(meta.uid(), rustix::process::getuid().as_raw(), meta.mode(), is_kind, kind) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not this user's own private terminal host", path.display()),
        ))
    }
}

fn private(owner: u32, me: u32, mode: u32, is_kind: bool, kind: Kind) -> bool {
    let closed = kind == Kind::Socket || mode & 0o077 == 0;
    owner == me && is_kind && closed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_private_folder_and_a_socket_of_this_user_are_trusted() {
        assert!(private(501, 501, 0o40700, true, Kind::Folder));
        assert!(!private(502, 501, 0o40700, true, Kind::Folder), "another user's folder");
        assert!(!private(501, 501, 0o40777, true, Kind::Folder), "a folder others can write to");
        assert!(!private(501, 501, 0o40750, true, Kind::Folder), "a folder others can read");
        assert!(!private(501, 501, 0o100600, false, Kind::Socket), "a plain file where the socket goes");
        assert!(!private(502, 501, 0o140600, true, Kind::Socket), "another user's socket");
        assert!(private(501, 501, 0o140600, true, Kind::Socket));
        assert!(private(501, 501, 0o100500, true, Kind::File));
        assert!(!private(501, 501, 0o100755, true, Kind::File), "a copy others can read");
        assert!(!private(501, 501, 0o100720, true, Kind::File), "a copy others can change");
        assert!(!private(502, 501, 0o100500, true, Kind::File), "another user's copy");
    }

    #[test]
    fn a_file_in_the_sockets_place_is_never_connected_to() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("terminal");
        private_folder(&folder).unwrap();
        std::fs::write(socket_path(&folder, PROTOCOL), "").unwrap();
        let error = Host::new(&folder, None).connect().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    }

    #[test]
    fn an_open_folder_of_this_user_is_closed_and_one_of_another_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("terminal");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(Host::new(&folder, None).connect().unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        private_folder(&folder).unwrap();
        assert_eq!(std::fs::metadata(&folder).unwrap().mode() & 0o777, 0o700);
        if rustix::process::getuid().as_raw() != 0 {
            assert!(private_folder(Path::new("/")).is_err(), "a folder this user does not own");
        }
    }

    #[test]
    fn a_long_folder_puts_its_socket_in_a_short_place() {
        let short = Path::new("/tmp/zen/terminal");
        assert_eq!(socket_path(short, 3), short.join("host-3.sock"));
        let long = PathBuf::from(format!("/tmp/{}/terminal", "x".repeat(120)));
        let socket = socket_path(&long, PROTOCOL);
        assert!(socket.as_os_str().len() <= SOCKET_LIMIT + 20, "{}", socket.display());
        assert_eq!(socket, socket_path(&long, PROTOCOL), "the same folder, the same socket");
        assert_ne!(socket, socket_path(&long.join("other"), PROTOCOL));
        assert_ne!(socket, socket_path(&long, PROTOCOL + 1), "each protocol its own socket");
    }
}
