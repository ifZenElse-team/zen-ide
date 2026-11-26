#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn language_servers_are_found_and_started_with_the_login_shell_s_path_and_variables() {
    let dir = tempfile::tempdir().unwrap();
    let tools = dir.path().join("login-tools");
    let variables = [
        ("PATH", format!("{}:/usr/bin:/bin", tools.display())),
        ("FROM_LOGIN", "login".to_owned()),
        ("SHARED", "login".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    zen_env::adopt(zen_env::Environment { shell: PathBuf::from("/bin/zsh"), variables });
    assert_eq!(zen_lsp::servers::search_folders().first(), Some(&tools), "the login shell's PATH comes first");
    let path = zen_lsp::servers::default_environment();
    assert!(path[0].1.to_string_lossy().starts_with(&tools.to_string_lossy().into_owned()), "{path:?}");

    let written = dir.path().join("env");
    let script = format!("printf '%s|%s' \"$FROM_LOGIN\" \"$SHARED\" > '{}'", written.display());
    let launcher = zen_lsp::process_launcher(
        PathBuf::from("/bin/sh"),
        vec!["-c".to_owned(), script],
        dir.path().to_owned(),
        vec![("SHARED".into(), "the server's".into())],
    );
    let _launched = launcher().unwrap();
    let started = Instant::now();
    while !written.exists() && started.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(std::fs::read_to_string(&written).unwrap(), "login|the server's");
}
