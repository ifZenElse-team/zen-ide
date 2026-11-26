#![cfg(unix)]

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zen_db::driver::{ContextKind, ErrorKind, ExecOptions};
use zen_db::engine::{ConnectRequest, Engine, OperationStatus, Timing, TransactionPolicy};
use zen_db::registry::{DataSource, SshSettings, TlsMode, TlsSettings};
use zen_db::ssh::SshConfig;
use zen_db::types::{ConnectionType, DEVELOPMENT};
use zen_db::value::Value;
use zen_secrets::Secret;

const WAIT: Duration = Duration::from_secs(30);

fn ports() -> Vec<u16> {
    std::env::var("ZEN_DB_PG_PORTS")
        .unwrap_or_else(|_| "54315,54318".to_owned())
        .split(',')
        .filter_map(|port| port.trim().parse().ok())
        .collect()
}

fn request(id: &str, port: u16, tls: TlsSettings, ssh: Option<SshSettings>) -> ConnectRequest {
    let source = DataSource {
        id: id.to_owned(),
        name: id.to_owned(),
        driver: "postgresql".to_owned(),
        connection_type: DEVELOPMENT.to_owned(),
        host: Some("127.0.0.1".to_owned()),
        port: Some(port),
        database: Some("postgres".to_owned()),
        user: Some("postgres".to_owned()),
        tls,
        ssh,
        ..DataSource::default()
    };
    let kind = ConnectionType::built_in().into_iter().find(|known| known.id == DEVELOPMENT).unwrap();
    ConnectRequest { source, kind, password: Some(Secret::new("zen".to_owned())), stored: None, ssh_secret: None }
}

fn ssl_in_use(engine: &Engine, id: &str) -> bool {
    let sql = "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()";
    let op = engine.execute(id, ContextKind::Main, sql, ExecOptions::default()).unwrap();
    let state = engine.wait(op, WAIT).unwrap();
    assert_eq!(state.status, OperationStatus::Done, "{state:?}");
    state.execution.unwrap().rows[0][0] == Value::Bool(true)
}

fn fake_ssh(dir: &Path) -> PathBuf {
    let script = dir.join("ssh");
    let text = format!(
        "#!/bin/sh\ncase \" $* \" in *\" -O \"*) exit 0;; *\" -G \"*) exit 0;; esac\necho \"$@\" > '{}'\nfor a in \"$@\"; do [ \"$p\" = -L ] && port=${{a#127.0.0.1:}} && port=${{port%%:*}}; p=$a; done\nuntil /usr/bin/nc -z 127.0.0.1 $port; do sleep 0.05; done\necho \"debug1: Local forwarding listening on 127.0.0.1 port $port.\" >&2\nexec sleep 600\n",
        dir.join("args").display()
    );
    std::fs::write(&script, text).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn serve_forward(args: PathBuf) {
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
        let parts: Vec<&str> = forward.split(':').collect();
        let local: u16 = parts[1].parse().unwrap();
        let target = format!("{}:{}", parts[2], parts[3]);
        let listener = TcpListener::bind(("127.0.0.1", local)).unwrap();
        for incoming in listener.incoming() {
            let Ok(client) = incoming else { break };
            let Ok(server) = TcpStream::connect(&target) else { continue };
            let (mut client_read, mut server_write) = (client.try_clone().unwrap(), server.try_clone().unwrap());
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut client_read, &mut server_write);
                let _ = server_write.shutdown(std::net::Shutdown::Write);
            });
            let (mut server_read, mut client_write) = (server, client);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut server_read, &mut client_write);
                let _ = client_write.flush();
                let _ = client_write.shutdown(std::net::Shutdown::Write);
            });
        }
    });
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn test_connection_and_sessions_go_through_the_tunnel() {
    for port in ports() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        engine.set_ssh(SshConfig { program: fake_ssh(dir.path()), askpass: None, folder: dir.path().join("s") });
        let ssh = SshSettings { host: "bastion".to_owned(), ..SshSettings::default() };
        let tls = TlsSettings { mode: TlsMode::Require, ..TlsSettings::default() };
        serve_forward(dir.path().join("args"));
        let op = engine.test(request("tunnel", port, tls.clone(), Some(ssh.clone()))).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Done, "{state:?}");
        assert!(state.test.unwrap().server_version.unwrap().starts_with("PostgreSQL"));
        let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
        assert!(args.contains(&format!(":127.0.0.1:{port}")) && args.ends_with("-- bastion\n"), "{args}");

        std::fs::remove_file(dir.path().join("args")).unwrap();
        serve_forward(dir.path().join("args"));
        let op = engine.connect(request("tunnel", port, tls, Some(ssh))).unwrap();
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
        assert!(ssl_in_use(&engine, "tunnel"), "TLS runs inside the tunnel");
        let op = engine.disconnect("tunnel", TransactionPolicy::Rollback).unwrap();
        assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done);
    }
}

#[test]
#[ignore = "needs PostgreSQL in Docker (docs/development.md)"]
fn every_tls_mode_does_what_it_says() {
    let ca = std::env::var_os("ZEN_DB_PG_CA").map(PathBuf::from);
    for port in ports() {
        let engine = Engine::with_timing(zen_db::drivers(), "Zen Test", Timing::default());
        let modes = [(TlsMode::Disable, false), (TlsMode::Prefer, true), (TlsMode::Require, true)];
        for (mode, encrypted) in modes {
            let id = format!("tls-{mode:?}");
            let tls = TlsSettings { mode, ..TlsSettings::default() };
            let op = engine.connect(request(&id, port, tls, None)).unwrap();
            assert_eq!(engine.wait(op, WAIT).unwrap().status, OperationStatus::Done, "{mode:?}");
            assert_eq!(ssl_in_use(&engine, &id), encrypted, "{mode:?}");
            let op = engine.disconnect(&id, TransactionPolicy::Rollback).unwrap();
            engine.wait(op, WAIT);
        }
        let untrusted = TlsSettings { mode: TlsMode::VerifyCa, ..TlsSettings::default() };
        let op = engine.test(request("tls-untrusted", port, untrusted, None)).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(
            state.error.map(|error| error.kind),
            Some(ErrorKind::Tls),
            "a self-signed certificate is not trusted"
        );
        let Some(ca) = &ca else { continue };
        let chain = TlsSettings { mode: TlsMode::VerifyCa, ca_file: Some(ca.clone()), ..TlsSettings::default() };
        let op = engine.connect(request("tls-ca", port, chain, None)).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(state.status, OperationStatus::Done, "verify-ca trusts its own certificate: {state:?}");
        assert!(ssl_in_use(&engine, "tls-ca"));
        let full = TlsSettings { mode: TlsMode::VerifyFull, ca_file: Some(ca.clone()), ..TlsSettings::default() };
        let op = engine.test(request("tls-full", port, full, None)).unwrap();
        let state = engine.wait(op, WAIT).unwrap();
        assert_eq!(
            state.error.map(|error| error.kind),
            Some(ErrorKind::Tls),
            "127.0.0.1 is not the certificate's name"
        );
    }
}
