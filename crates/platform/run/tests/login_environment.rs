#![cfg(unix)]

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zen_run::{CommandLine, Process, ProcessEvent, ProcessSink};

fn output(line: &CommandLine) -> String {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    let sink: ProcessSink = Arc::new(move |event| {
        let _ = sender.lock().unwrap().send(event);
    });
    let _process = Process::spawn(line, sink).unwrap();
    let mut text = String::new();
    loop {
        match receiver.recv_timeout(Duration::from_secs(20)).unwrap() {
            ProcessEvent::Output { text: line, .. } => text.push_str(&line),
            ProcessEvent::Exited { .. } => return text,
        }
    }
}

#[test]
fn a_run_gets_the_login_shell_s_variables_under_its_own() {
    let line = CommandLine {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), "printf '%s|%s|%s' \"$FROM_LOGIN\" \"$SHARED\" \"$KUBECONFIG\"".to_owned()],
        env: vec![("SHARED".to_owned(), "the configuration's".to_owned())],
        ..CommandLine::default()
    };
    let variables = [("FROM_LOGIN", "login"), ("SHARED", "login"), ("KUBECONFIG", "/k/a:/k/b")]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    zen_env::adopt(zen_env::Environment { shell: PathBuf::from("/bin/zsh"), variables });
    assert_eq!(output(&line).trim(), "login|the configuration's|/k/a:/k/b");
    zen_env::set_enabled(false);
    assert_eq!(output(&line).trim(), "|the configuration's|", "off: the IDE's own environment, as before");
    zen_env::set_enabled(true);
}
