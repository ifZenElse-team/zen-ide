use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::memory::{ID, MemoryDriver};
use crate::registry::{AuthKind, SshSettings};
use crate::ssh::SshConfig;
use crate::types::DEVELOPMENT;

const WAIT: Duration = Duration::from_secs(10);

struct Fake {
    dir: tempfile::TempDir,
    engine: Engine,
    driver: Arc<MemoryDriver>,
}

fn start(body: &str) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" -O \"*) exit 0;; *\" -G \"*) echo 'hostname db-bastion.internal'; exit 0;; esac\necho $$ > '{pid}'\n{body}\necho \"$@\" > '{args}'\nfor a in \"$@\"; do [ \"$p\" = -L ] && port=${{a#127.0.0.1:}} && port=${{port%%:*}}; p=$a; done\necho \"debug1: Local forwarding listening on 127.0.0.1 port $port.\" >&2\nexec sleep 600\n",
        args = root.join("args").display(),
        pid = root.join("pid").display(),
    );
    write_script(&root.join("ssh"), &script);
    let askpass = "#!/bin/sh\nprintf '{\"token\":\"%s\",\"prompt\":\"%s\"}\\n' \"$ZEN_DB_ASKPASS_TOKEN\" \"$1\" | /usr/bin/nc -U \"$ZEN_DB_ASKPASS\" | sed -n 's/.*\"answer\":\"\\([^\"]*\\)\".*/\\1/p'\n";
    write_script(&root.join("askpass"), askpass);
    let folder = root.join("s");
    let driver = Arc::new(MemoryDriver::new(false));
    let timing = Timing { backoff: vec![Duration::ZERO], connect_timeout: Duration::from_secs(5), ..Timing::default() };
    let engine = Engine::with_timing(vec![Arc::clone(&driver) as Arc<dyn Driver>], "Zen Test", timing);
    engine.set_ssh(SshConfig { program: root.join("ssh"), askpass: Some(root.join("askpass")), folder });
    Fake { dir, engine, driver }
}

fn write_script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn request(id: &str) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: format!("Name of {id}"),
        driver: ID.to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        auth: AuthKind::Password,
        host: Some("db.internal".to_owned()),
        port: Some(6379),
        ssh: Some(SshSettings {
            host: "bastion".to_owned(),
            user: Some("deploy".to_owned()),
            port: None,
            jump: Some("jump1".to_owned()),
        }),
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("pw".to_owned())), stored: None, ssh_secret: None }
}

impl Fake {
    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn listen(&self) -> std::thread::JoinHandle<()> {
        let args = self.file("args");
        std::thread::spawn(move || {
            let deadline = Instant::now() + WAIT;
            let text = loop {
                if let Ok(text) = std::fs::read_to_string(&args)
                    && text.ends_with('\n')
                {
                    break text;
                }
                assert!(Instant::now() < deadline, "the fake ssh never started");
                std::thread::sleep(Duration::from_millis(10));
            };
            let forward = text.split_whitespace().skip_while(|arg| *arg != "-L").nth(1).unwrap().to_owned();
            let port: u16 = forward.split(':').nth(1).unwrap().parse().unwrap();
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let until = Instant::now() + WAIT;
            while Instant::now() < until {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.read(&mut [0u8; 16]);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        })
    }

    fn pid(&self) -> String {
        std::fs::read_to_string(self.file("pid")).unwrap().trim().to_owned()
    }

    fn connect(&self, request: ConnectRequest) -> OperationState {
        let op = self.engine.connect(request).unwrap();
        self.engine.wait(op, WAIT * 2).unwrap()
    }
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_connection_through_ssh_opens_its_tunnel_first_and_closes_it_with_the_connection() {
    let fake = start("");
    let _listening = fake.listen();
    let state = fake.connect(request("t"));
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    let args = std::fs::read_to_string(fake.file("args")).unwrap();
    for part in ["-N -L 127.0.0.1:", ":db.internal:6379", "-l deploy", "-J jump1", "-- bastion"] {
        assert!(args.contains(part), "{part} in {args}");
    }
    let port: u16 = args
        .split_whitespace()
        .skip_while(|arg| *arg != "-L")
        .nth(1)
        .unwrap()
        .split(':')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let opened = fake.driver.server.opened();
    assert!(
        !opened.is_empty() && opened.iter().all(|open| open.via == Some(SocketAddr::from(([127, 0, 0, 1], port)))),
        "{opened:?}"
    );
    let pid = fake.pid();
    assert!(alive(&pid));
    let op = fake.engine.disconnect("t", TransactionPolicy::Rollback).unwrap();
    assert_eq!(fake.engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    wait_until("ssh stops with the connection", || !alive(&pid));
}

#[test]
fn what_ssh_said_is_the_error_when_the_tunnel_does_not_open() {
    let fake = start(
        "echo 'ssh: Could not resolve hostname bastion: nodename nor servname provided, or not known' >&2\nexit 255",
    );
    let state = fake.connect(request("dns"));
    assert_eq!(state.status, OperationStatus::Failed);
    let error = state.error.unwrap();
    assert_eq!((error.kind, error.code.as_deref()), (ErrorKind::Connection, Some("ssh")));
    assert!(error.detail.contains("Could not resolve hostname bastion"), "{error:?}");
    assert!(fake.driver.server.opened().is_empty(), "no session opens without the tunnel");
    assert_eq!(fake.engine.status("dns").state, ConnectionState::Failed);
}

fn asking(prompt: &str) -> String {
    format!(
        "answer=$(\"$SSH_ASKPASS\" \"{prompt}\")\nif [ \"$answer\" != \"s3cret\" ]; then echo 'deploy@bastion: Permission denied (publickey,password).' >&2; exit 255; fi"
    )
}

#[test]
fn a_saved_ssh_password_answers_once_and_otherwise_the_person_is_asked() {
    let fake = start(&asking("deploy@bastion's password: "));
    let _listening = fake.listen();
    let store = Arc::new(zen_secrets::MemoryStore::new());
    let id = zen_secrets::database::id(None, "saved", zen_secrets::database::Kind::Ssh).unwrap();
    store.set(&id, &Secret::new("s3cret".to_owned())).unwrap();
    let mut saved = request("saved");
    saved.ssh_secret = Some(StoredSecret { store: Arc::clone(&store) as Arc<dyn SecretStore>, id });
    let state = fake.connect(saved);
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert!(fake.engine.ssh_prompts().is_empty());

    let second = start(&asking("Enter passphrase for key '/Users/me/.ssh/id_ed25519': "));
    let _listening = second.listen();
    let id = zen_secrets::database::id(None, "asked", zen_secrets::database::Kind::Ssh).unwrap();
    let mut asked = request("asked");
    asked.ssh_secret = Some(StoredSecret { store: Arc::clone(&store) as Arc<dyn SecretStore>, id: id.clone() });
    let op = second.engine.connect(asked).unwrap();
    wait_until("ssh asks the person", || !second.engine.ssh_prompts().is_empty());
    let prompt = second.engine.ssh_prompts().remove(0);
    assert!(prompt.secret && !prompt.question, "{prompt:?}");
    assert_eq!(prompt.connection, "asked");
    assert!(second.engine.take_events().iter().any(|event| matches!(event, Event::SshPrompt { .. })));
    assert!(second.engine.answer_ssh(prompt.id, Some(Secret::new("s3cret".to_owned())), true));
    assert_eq!(second.engine.wait(op, WAIT * 2).unwrap().status, OperationStatus::Done);
    assert_eq!(
        store.get(&id).unwrap().map(|secret| secret.expose().to_owned()).as_deref(),
        Some("s3cret"),
        "kept as asked"
    );
}

#[test]
fn an_unknown_host_key_is_never_accepted_by_itself() {
    let fake = start(&format!("{}\n", asking("Are you sure you want to continue connecting (yes/no/[fingerprint])? ")));
    let op = fake.engine.connect(request("hostkey")).unwrap();
    wait_until("ssh asks about the host key", || !fake.engine.ssh_prompts().is_empty());
    let prompt = fake.engine.ssh_prompts().remove(0);
    assert!(prompt.question && !prompt.secret, "{prompt:?}");
    assert!(fake.engine.answer_ssh(prompt.id, None, false));
    let state = fake.engine.wait(op, WAIT * 2).unwrap();
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    assert!(state.error.unwrap().detail.contains("Permission denied"));
}

#[test]
fn a_saved_ssh_password_never_answers_a_jump_host() {
    let script = "jumped=$(\"$SSH_ASKPASS\" \"deploy@jump1's password: \")\n\
                  if [ \"$jumped\" != \"jumpw\" ]; then echo 'deploy@jump1: Permission denied (password).' >&2; exit 255; fi\n\
                  answer=$(\"$SSH_ASKPASS\" \"deploy@bastion's password: \")\n\
                  if [ \"$answer\" != \"s3cret\" ]; then echo 'deploy@bastion: Permission denied (password).' >&2; exit 255; fi";
    let fake = start(script);
    let _listening = fake.listen();
    let store = Arc::new(zen_secrets::MemoryStore::new());
    let id = zen_secrets::database::id(None, "jumped", zen_secrets::database::Kind::Ssh).unwrap();
    store.set(&id, &Secret::new("s3cret".to_owned())).unwrap();
    let mut saved = request("jumped");
    saved.ssh_secret = Some(StoredSecret { store: Arc::clone(&store) as Arc<dyn SecretStore>, id: id.clone() });
    let op = fake.engine.connect(saved).unwrap();
    wait_until("ssh asks the person for the jump host", || {
        !fake.engine.ssh_prompts().is_empty()
            || fake.engine.wait(op, Duration::ZERO).is_some_and(|state| state.status == OperationStatus::Failed)
    });
    let prompt = fake.engine.ssh_prompts().pop().expect("the jump host's password is asked, not sent");
    assert!(prompt.prompt.contains("jump1"), "{prompt:?}");
    assert!(fake.engine.answer_ssh(prompt.id, Some(Secret::new("jumpw".to_owned())), true));
    let state = fake.engine.wait(op, WAIT * 2).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    assert_eq!(
        store.get(&id).unwrap().map(|secret| secret.expose().to_owned()).as_deref(),
        Some("s3cret"),
        "the jump host's password is not kept as the server's"
    );
}

#[test]
fn a_foreign_listener_on_the_chosen_port_is_not_taken_for_the_tunnel() {
    let fake = start("echo \"$@\" > \"${0%/*}/args\"\nexec sleep 600");
    let _foreign = fake.listen();
    let op = fake.engine.connect(request("foreign")).unwrap();
    wait_until("the foreign listener is up", || std::fs::read_to_string(fake.file("args")).is_ok());
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        let state = fake.engine.wait(op, Duration::from_millis(100)).unwrap();
        assert_eq!(state.status, OperationStatus::Running, "a port that answers is not ssh's forward: {state:?}");
        assert!(fake.driver.server.opened().is_empty(), "the driver never connects, so no credentials leave");
    }
    let closed = fake.engine.disconnect("foreign", TransactionPolicy::Rollback).unwrap();
    let _ = fake.engine.wait(closed, WAIT);
    assert!(fake.driver.server.opened().is_empty());
}

#[test]
fn ssh_that_exits_before_it_reports_the_forward_fails_the_connection() {
    let fake = start("exit 0");
    let state = fake.connect(request("gone"));
    assert_eq!(state.status, OperationStatus::Failed, "{state:?}");
    assert_eq!(state.error.unwrap().code.as_deref(), Some("ssh"));
    assert!(fake.driver.server.opened().is_empty());
}
