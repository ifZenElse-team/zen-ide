use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::driver::{DbError, ErrorKind};
use crate::registry::SshSettings;

pub const ASKPASS_SOCKET: &str = "ZEN_DB_ASKPASS";

pub const ASKPASS_TOKEN: &str = "ZEN_DB_ASKPASS_TOKEN";

const STDERR_KEPT: usize = 4096;

pub const SSH_FAILED: &str = "ssh";

#[derive(Clone, Debug)]
pub struct SshConfig {
    pub program: PathBuf,
    pub askpass: Option<PathBuf>,
    pub folder: PathBuf,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self { program: find_ssh(), askpass: std::env::current_exe().ok(), folder: short_folder() }
    }
}

fn find_ssh() -> PathBuf {
    std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .filter(|folder| folder.is_absolute())
        .map(|folder| folder.join("ssh"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("/usr/bin/ssh"))
}

#[cfg(unix)]
fn short_folder() -> PathBuf {
    PathBuf::from(format!("/tmp/zen-ssh-{}", rustix::process::geteuid().as_raw()))
}

#[cfg(not(unix))]
fn short_folder() -> PathBuf {
    std::env::temp_dir().join("zen-ssh")
}

pub fn private_folder(folder: &Path) -> Result<(), DbError> {
    let failed = |error: std::io::Error| DbError::new(ErrorKind::Invalid, format!("{}: {error}", folder.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        match std::fs::DirBuilder::new().mode(0o700).create(folder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(failed(error)),
        }
        let meta = std::fs::symlink_metadata(folder).map_err(failed)?;
        if !meta.is_dir() || meta.uid() != rustix::process::geteuid().as_raw() || meta.permissions().mode() & 0o077 != 0
        {
            return Err(DbError::new(ErrorKind::Invalid, format!("{} is not a private folder", folder.display())));
        }
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(folder).map_err(failed)?;
    Ok(())
}

fn plain(value: &str, what: &str) -> Result<(), DbError> {
    let bad = value.is_empty()
        || value.starts_with('-')
        || value.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\'' || c == '"');
    if bad { Err(DbError::new(ErrorKind::Invalid, format!("ssh {what}: {value:?}"))) } else { Ok(()) }
}

fn forward(local: u16, (host, port): (&str, u16)) -> String {
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_owned() };
    format!("127.0.0.1:{local}:{host}:{port}")
}

pub fn tunnel_args(
    settings: &SshSettings,
    local: u16,
    target: (&str, u16),
    timeout: Duration,
) -> Result<Vec<OsString>, DbError> {
    plain(settings.host.trim(), "host")?;
    plain(target.0, "database host")?;
    let mut args: Vec<OsString> = Vec::new();
    let mut option = |text: String| {
        args.push("-o".into());
        args.push(text.into());
    };
    option("ExitOnForwardFailure=yes".to_owned());
    option("BatchMode=no".to_owned());
    option("RequestTTY=no".to_owned());
    option(format!("ConnectTimeout={}", timeout.as_secs().max(1)));
    option("ServerAliveInterval=15".to_owned());
    option("ServerAliveCountMax=3".to_owned());
    option("ControlMaster=no".to_owned());
    option("ControlPath=none".to_owned());
    args.push("-v".into());
    args.push("-N".into());
    args.push("-L".into());
    args.push(forward(local, target).into());
    if let Some(user) = settings.user.as_deref().map(str::trim).filter(|user| !user.is_empty()) {
        plain(user, "user")?;
        args.push("-l".into());
        args.push(user.into());
    }
    if let Some(port) = settings.port {
        args.push("-p".into());
        args.push(port.to_string().into());
    }
    if let Some(jump) = settings.jump.as_deref().map(str::trim).filter(|jump| !jump.is_empty()) {
        for hop in jump.split(',') {
            plain(hop.trim(), "jump host")?;
        }
        args.push("-J".into());
        args.push(jump.into());
    }
    args.push("--".into());
    args.push(settings.host.trim().into());
    Ok(args)
}

pub fn clean_stderr(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && !line.starts_with("Warning: Permanently added") && !line.starts_with("debug1:")
        })
        .collect();
    let start = lines.len().saturating_sub(6);
    lines[start..].join("\n")
}

pub fn reports_forward(line: &str, port: u16) -> bool {
    let Some(rest) = line.trim().strip_prefix("debug1: Local forwarding listening on 127.0.0.1 port ") else {
        return false;
    };
    rest.trim_end_matches('.') == port.to_string()
}

fn free_port() -> Result<u16, DbError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| DbError::new(ErrorKind::Connection, error.to_string()))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| DbError::new(ErrorKind::Connection, error.to_string()))
}

pub struct Tunnel {
    address: SocketAddr,
    child: Mutex<Option<Child>>,
    stderr: Arc<Mutex<String>>,
    settings: SshSettings,
}

impl std::fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tunnel({} via {})", self.address, self.settings.host)
    }
}

impl Tunnel {
    pub async fn open(
        config: &SshConfig,
        settings: &SshSettings,
        target: (&str, u16),
        askpass: Option<(&Path, &str)>,
        timeout: Duration,
    ) -> Result<Tunnel, DbError> {
        private_folder(&config.folder)?;
        let port = free_port()?;
        let args = tunnel_args(settings, port, target, timeout)?;
        let mut command = Command::new(&config.program);
        command.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        if let (Some(helper), Some((socket, token))) = (&config.askpass, askpass) {
            command
                .env("SSH_ASKPASS", helper)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(ASKPASS_SOCKET, socket)
                .env(ASKPASS_TOKEN, token);
        }
        let mut child = command.spawn().map_err(|error| {
            DbError::new(ErrorKind::Connection, format!("{}: {error}", config.program.display())).with_code(SSH_FAILED)
        })?;
        let stderr = Arc::new(Mutex::new(String::new()));
        let forwarding = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Some(mut pipe) = child.stderr.take() {
            let kept = Arc::clone(&stderr);
            let reported = Arc::clone(&forwarding);
            tokio::spawn(async move {
                let mut buffer = [0u8; 1024];
                let mut line = Vec::new();
                while let Ok(read) = pipe.read(&mut buffer).await {
                    if read == 0 {
                        break;
                    }
                    for byte in &buffer[..read] {
                        if *byte == b'\n' {
                            if reports_forward(&String::from_utf8_lossy(&line), port) {
                                reported.store(true, std::sync::atomic::Ordering::SeqCst);
                            }
                            line.clear();
                        } else if line.len() < 512 {
                            line.push(*byte);
                        }
                    }
                    let mut text = kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    text.push_str(&String::from_utf8_lossy(&buffer[..read]));
                    if text.len() > STDERR_KEPT {
                        let cut = text.len() - STDERR_KEPT;
                        let cut = (cut..text.len()).find(|at| text.is_char_boundary(*at)).unwrap_or(text.len());
                        text.drain(..cut);
                    }
                }
            });
        }
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let tunnel = Tunnel { address, child: Mutex::new(Some(child)), stderr, settings: settings.clone() };
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = tunnel.exited() {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let said = clean_stderr(&tunnel.stderr());
                let detail = if said.is_empty() { format!("ssh exited with {status}") } else { said };
                return Err(DbError::new(ErrorKind::Connection, detail).with_code(SSH_FAILED));
            }
            if forwarding.load(std::sync::atomic::Ordering::SeqCst) {
                return Ok(tunnel);
            }
            if Instant::now() >= deadline {
                tunnel.close().await;
                let said = clean_stderr(&tunnel.stderr());
                return Err(DbError::new(ErrorKind::Timeout, said).with_code(SSH_FAILED));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn stderr(&self) -> String {
        self.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn exited(&self) -> Option<std::process::ExitStatus> {
        let mut child = self.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match child.as_mut().map(Child::try_wait) {
            Some(Ok(Some(status))) => Some(status),
            Some(Ok(None)) => None,
            Some(Err(_)) | None => Some(std::process::ExitStatus::default()),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.exited().is_none()
    }

    pub async fn close(&self) {
        let child = self.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(mut child) = child {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AskRequest {
    pub token: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AskReply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

pub fn askpass_main(args: &[OsString]) -> Option<i32> {
    let socket = std::env::var_os(ASKPASS_SOCKET)?;
    let token = std::env::var(ASKPASS_TOKEN).unwrap_or_default();
    let prompt = args.first().map(|prompt| prompt.to_string_lossy().into_owned()).unwrap_or_default();
    Some(match ask(Path::new(&socket), &AskRequest { token, prompt }) {
        Some(answer) => {
            let mut stdout = std::io::stdout();
            let written = writeln!(stdout, "{answer}").and_then(|()| stdout.flush());
            if written.is_ok() { 0 } else { 1 }
        }
        None => 1,
    })
}

pub const TCP_CHANNEL: &str = "tcp:";

pub fn ask(socket: &Path, request: &AskRequest) -> Option<String> {
    let text = socket.to_string_lossy();
    match text.strip_prefix(TCP_CHANNEL) {
        Some(address) => {
            let address: SocketAddr = address.parse().ok()?;
            if !address.ip().is_loopback() {
                return None;
            }
            exchange(std::net::TcpStream::connect(address).ok()?, request)
        }
        #[cfg(unix)]
        None => exchange(std::os::unix::net::UnixStream::connect(socket).ok()?, request),
        #[cfg(not(unix))]
        None => None,
    }
}

fn exchange<S: std::io::Read + Write>(mut stream: S, request: &AskRequest) -> Option<String> {
    let mut line = serde_json::to_string(request).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    let mut reader = std::io::BufReader::new(stream);
    let mut answer = String::new();
    reader.read_line(&mut answer).ok()?;
    serde_json::from_str::<AskReply>(&answer).ok()?.answer
}

#[cfg(test)]
pub(crate) mod tests;
