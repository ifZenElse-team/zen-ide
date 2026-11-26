use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

use super::{ConnectRequest, Engine, Event, StoredSecret, Waiting, runtime};
use crate::driver::{DbError, ErrorKind};
use crate::ssh::{AskReply, AskRequest, SshConfig, Tunnel};
use zen_secrets::Secret;

const ANSWER_WAIT: Duration = Duration::from_secs(180);

const LONGEST_LINE: u64 = 16 * 1024;

/// A question ssh asks while a tunnel opens, waiting for the person (never with an answer).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshPrompt {
    /// Its id, for answering it.
    pub id: u64,
    /// The connection whose tunnel asks.
    pub connection: String,
    /// The connection's name.
    pub name: String,
    /// What ssh asks, as it wrote it.
    pub prompt: String,
    /// Whether the answer is a secret (a password or a key's passphrase).
    pub secret: bool,
    /// Whether it is a yes-or-no question, such as accepting an unknown host key.
    pub question: bool,
}

struct Asker {
    connection: String,
    name: String,
    stored: Option<StoredSecret>,
    tried: bool,
    hosts: Vec<String>,
    jumps: bool,
}

impl Asker {
    fn saved_for(&self, prompt: &str) -> bool {
        storable(prompt)
            && prompt_host(prompt).map_or(!self.jumps || is_passphrase(prompt), |host| {
                self.hosts.iter().any(|known| known.eq_ignore_ascii_case(host))
            })
    }
}

struct Resolved {
    hostname: Option<String>,
    jumps: bool,
}

fn resolved_from(printed: &str) -> Resolved {
    let value = |key: &str| {
        printed.lines().find_map(|line| {
            let (name, value) = line.trim().split_once(' ')?;
            name.eq_ignore_ascii_case(key).then(|| value.trim().to_owned())
        })
    };
    let jumps = value("proxyjump").is_some_and(|jump| !jump.is_empty() && !jump.eq_ignore_ascii_case("none"));
    Resolved { hostname: value("hostname"), jumps }
}

async fn resolve(program: &std::path::Path, destination: &str) -> Resolved {
    let host = ssh_host(destination);
    if host.starts_with('-') || host.contains(char::is_whitespace) {
        return Resolved { hostname: None, jumps: false };
    }
    let mut command = tokio::process::Command::new(program);
    command.args(["-G", "--", &host]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    match tokio::time::timeout(Duration::from_secs(5), command.output()).await {
        Ok(Ok(output)) if output.status.success() => resolved_from(&String::from_utf8_lossy(&output.stdout)),
        _ => Resolved { hostname: None, jumps: false },
    }
}

type Reply = oneshot::Sender<Option<(Secret, bool)>>;

pub(super) struct Askpass {
    path: PathBuf,
    askers: Mutex<HashMap<String, Asker>>,
    pending: Mutex<BTreeMap<u64, (SshPrompt, Reply)>>,
    next: AtomicU64,
}

impl Drop for Askpass {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |at| at.as_nanos()));
    let first = hasher.finish();
    let mut second = std::collections::hash_map::RandomState::new().build_hasher();
    second.write_u64(first);
    format!("{first:016x}{:016x}", second.finish())
}

fn is_secret(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    ["password", "passphrase", "pin for", "verification code", "one-time"].iter().any(|word| lower.contains(word))
}

fn is_question(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    lower.contains("(yes/no") || lower.contains("yes/no/")
}

fn storable(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    lower.contains("password") || lower.contains("passphrase")
}

fn is_passphrase(prompt: &str) -> bool {
    prompt.to_ascii_lowercase().contains("passphrase")
}

fn prompt_host(prompt: &str) -> Option<&str> {
    let prompt = prompt.trim();
    let named = match prompt.strip_prefix('(') {
        Some(rest) => rest.split_once(')')?.0,
        None => prompt.split_once("'s password")?.0,
    };
    Some(named.rsplit_once('@').map_or(named, |(_, host)| host))
}

fn ssh_host(destination: &str) -> String {
    let destination = destination.trim();
    destination.rsplit_once('@').map_or(destination, |(_, host)| host).to_owned()
}

impl Engine {
    pub fn set_ssh(&self, config: SshConfig) {
        *self.shared.ssh.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    }

    fn ssh_config(&self) -> SshConfig {
        self.shared.ssh.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn askpass(&self) -> Result<Arc<Askpass>, DbError> {
        let mut slot = self.shared.askpass.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(askpass) = slot.as_ref() {
            return Ok(Arc::clone(askpass));
        }
        let config = self.ssh_config();
        crate::ssh::private_folder(&config.folder)?;
        let failed =
            |what: &str, error: std::io::Error| DbError::new(ErrorKind::Connection, format!("{what}: {error}"));
        let _entered = runtime().enter();
        #[cfg(unix)]
        let (path, listener) = {
            let path = config.folder.join(format!("ask-{}-{}.sock", std::process::id(), self.shared.id));
            let _ = std::fs::remove_file(&path);
            let listener =
                tokio::net::UnixListener::bind(&path).map_err(|error| failed(&path.display().to_string(), error))?;
            (path, listener)
        };
        #[cfg(not(unix))]
        let (path, listener) = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
                .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
                .and_then(tokio::net::TcpListener::from_std)
                .map_err(|error| failed("127.0.0.1", error))?;
            let address = listener.local_addr().map_err(|error| failed("127.0.0.1", error))?;
            (PathBuf::from(format!("{}{address}", crate::ssh::TCP_CHANNEL)), listener)
        };
        let askpass =
            Arc::new(Askpass { path, askers: Mutex::default(), pending: Mutex::default(), next: AtomicU64::new(0) });
        let weak = Arc::downgrade(&self.shared);
        let served = Arc::clone(&askpass);
        runtime().spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let Some(shared) = weak.upgrade() else { break };
                let engine = Engine { shared };
                let askpass = Arc::clone(&served);
                tokio::spawn(async move { engine.answer_helper(&askpass, stream).await });
            }
        });
        *slot = Some(Arc::clone(&askpass));
        Ok(askpass)
    }

    async fn answer_helper<S>(&self, askpass: &Askpass, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let (read, mut write) = tokio::io::split(stream);
        let mut line = String::new();
        let mut reader = BufReader::new(tokio::io::AsyncReadExt::take(read, LONGEST_LINE));
        if reader.read_line(&mut line).await.is_err() {
            return;
        }
        let Ok(request) = serde_json::from_str::<AskRequest>(&line) else { return };
        let answer = self.answer_for(askpass, &request).await;
        let reply = AskReply { answer: answer.as_ref().map(|secret| secret.expose().to_owned()) };
        if let Ok(mut text) = serde_json::to_string(&reply) {
            text.push('\n');
            let _ = write.write_all(text.as_bytes()).await;
            let _ = write.shutdown().await;
        }
    }

    async fn answer_for(&self, askpass: &Askpass, request: &AskRequest) -> Option<Secret> {
        let (connection, name, stored) = {
            let mut askers = askpass.askers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let asker = askers.get_mut(&request.token)?;
            let stored = asker.stored.clone().filter(|_| !asker.tried && asker.saved_for(&request.prompt));
            asker.tried |= stored.is_some();
            (asker.connection.clone(), asker.name.clone(), stored)
        };
        if let Some(stored) = stored.clone()
            && let Ok(secret) = self.read_secret(stored).await
        {
            return Some(secret);
        }
        let id = askpass.next.fetch_add(1, Ordering::SeqCst) + 1;
        let prompt = SshPrompt {
            id,
            connection,
            name,
            prompt: request.prompt.trim().to_owned(),
            secret: is_secret(&request.prompt),
            question: is_question(&request.prompt),
        };
        let (sender, receiver) = oneshot::channel();
        askpass.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id, (prompt.clone(), sender));
        self.push_event(Event::SshPrompt { prompt: prompt.clone() });
        let answered = tokio::time::timeout(ANSWER_WAIT, receiver).await;
        askpass.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id);
        self.wake();
        let (secret, save) = answered.ok()?.ok()??;
        let store_to = {
            let askers = askpass.askers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            askers
                .get(&request.token)
                .filter(|asker| save && asker.saved_for(&prompt.prompt))
                .and_then(|asker| asker.stored.clone())
        };
        if let Some(stored) = store_to {
            let value = secret.clone();
            let _order = Arc::clone(&self.shared.secrets).lock_owned().await;
            let kept =
                tokio::task::spawn_blocking(move || stored.store.set(&stored.id, &value).map_err(|e| e.to_string()))
                    .await
                    .unwrap_or_else(|error| Err(error.to_string()));
            if let Err(detail) = kept {
                self.push_event(Event::SecretFailed { connection: prompt.connection.clone(), stored: false, detail });
            }
        }
        Some(secret)
    }

    pub fn ssh_prompts(&self) -> Vec<SshPrompt> {
        let slot = self.shared.askpass.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.as_ref().map_or_else(Vec::new, |askpass| {
            askpass
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .values()
                .map(|(prompt, _)| prompt.clone())
                .collect()
        })
    }

    pub fn answer_ssh(&self, id: u64, answer: Option<Secret>, save: bool) -> bool {
        let askpass = self.shared.askpass.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let Some(askpass) = askpass else { return false };
        let waiting = askpass.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id);
        match waiting {
            Some((_, reply)) => reply.send(answer.map(|secret| (secret, save))).is_ok(),
            None => false,
        }
    }

    pub(super) async fn open_tunnel(&self, request: &ConnectRequest) -> Result<Option<(Tunnel, String)>, DbError> {
        let Some(settings) = request.source.ssh.clone().filter(|ssh| !ssh.host.trim().is_empty()) else {
            return Ok(None);
        };
        let host = request.source.host.clone().unwrap_or_else(|| "localhost".to_owned());
        let port = request
            .source
            .port
            .or_else(|| self.driver(&request.source.driver).and_then(|driver| driver.default_port()))
            .ok_or_else(|| DbError::new(ErrorKind::Invalid, "port"))?;
        let config = self.ssh_config();
        let askpass = self.askpass()?;
        let resolved = resolve(&config.program, &settings.host).await;
        let mut hosts = vec![ssh_host(&settings.host)];
        hosts.extend(resolved.hostname.filter(|hostname| !hosts.contains(hostname)));
        let token = token();
        askpass.askers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(
            token.clone(),
            Asker {
                connection: request.source.id.clone(),
                name: request.source.name.clone(),
                stored: request.ssh_secret.clone(),
                tried: false,
                hosts,
                jumps: resolved.jumps || settings.jump.as_deref().is_some_and(|jump| !jump.trim().is_empty()),
            },
        );
        let timeout = self.shared.timing.connect_timeout + ANSWER_WAIT;
        let opened =
            Tunnel::open(&config, &settings, (&host, port), Some((askpass.path.as_path(), token.as_str())), timeout)
                .await;
        match opened {
            Ok(tunnel) => Ok(Some((tunnel, token))),
            Err(error) => {
                self.forget_token(&token);
                Err(error)
            }
        }
    }

    pub(super) fn forget_token(&self, token: &str) {
        if let Some(askpass) = self.shared.askpass.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() {
            askpass.askers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(token);
        }
    }

    pub fn tunnel_open(&self, id: &str) -> Option<SocketAddr> {
        let slot = Arc::clone(&self.state().connections.get(id)?.tunnel);
        let slot = slot.try_lock().ok()?;
        slot.as_ref().filter(|(tunnel, _)| tunnel.is_alive()).map(|(tunnel, _)| tunnel.address())
    }

    pub(super) async fn tunnel_address(&self, id: &str) -> Result<Option<SocketAddr>, DbError> {
        let (request, slot) = {
            let state = self.state();
            let Some(conn) = state.connections.get(id) else { return Ok(None) };
            (conn.request.clone(), Arc::clone(&conn.tunnel))
        };
        if request.source.ssh.as_ref().is_none_or(|ssh| ssh.host.trim().is_empty()) {
            return Ok(None);
        }
        let mut slot = slot.lock().await;
        if let Some((tunnel, _)) = slot.as_ref()
            && tunnel.is_alive()
        {
            return Ok(Some(tunnel.address()));
        }
        if let Some((tunnel, token)) = slot.take() {
            tunnel.close().await;
            self.forget_token(&token);
        }
        self.waiting(id, Waiting::Tunnel);
        let opened = self.open_tunnel(&request).await?;
        let address = opened.as_ref().map(|(tunnel, _)| tunnel.address());
        *slot = opened;
        Ok(address)
    }

    pub(super) fn close_tunnel(&self, slot: super::TunnelSlot) {
        let engine = self.clone();
        runtime().spawn(async move {
            let taken = slot.lock().await.take();
            if let Some((tunnel, token)) = taken {
                tunnel.close().await;
                engine.forget_token(&token);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asker(hosts: &[&str], jumps: bool) -> Asker {
        Asker {
            connection: "c".to_owned(),
            name: "c".to_owned(),
            stored: None,
            tried: false,
            hosts: hosts.iter().map(|host| (*host).to_owned()).collect(),
            jumps,
        }
    }

    #[test]
    fn a_saved_password_answers_only_the_server_whatever_names_a_jump_host() {
        let printed = "user ann\nhostname 10.0.0.5\nproxyjump bastion.example.com\nport 22\n";
        let resolved = resolved_from(printed);
        assert_eq!((resolved.hostname.as_deref(), resolved.jumps), (Some("10.0.0.5"), true));
        assert!(!resolved_from("hostname db\nproxyjump none\n").jumps);
        let through_config = asker(&["prod", "10.0.0.5"], resolved.jumps);
        assert!(!through_config.saved_for("(ann@bastion.example.com) Password: "), "a jump from ~/.ssh/config");
        assert!(!through_config.saved_for("ann@bastion.example.com's password: "));
        assert!(through_config.saved_for("(ann@10.0.0.5) Password: "), "the server under its HostName");
        assert!(!through_config.saved_for("Password: "), "a prompt naming no host could be the jump host's");
        let alias = asker(&["prod", "10.0.0.5"], false);
        assert!(alias.saved_for("ann@10.0.0.5's password: "), "an alias whose HostName differs");
        assert!(alias.saved_for("Password: "));
    }
}
