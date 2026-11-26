use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SETTLE: Duration = Duration::from_millis(10);

pub const PIPE_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct Captured {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn capture(command: &mut Command, timeout: Duration) -> Result<Captured, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    capture_child(child, timeout)
        .ok_or_else(|| zen_i18n::t!("run-no-answer", program = program, seconds = timeout.as_secs()))
}

pub fn capture_child(mut child: Child, timeout: Duration) -> Option<Captured> {
    let stdout = Collected::start(child.stdout.take());
    let stderr = Collected::start(child.stderr.take());
    let status = wait_within(&mut child, timeout)?;
    let deadline = Instant::now() + PIPE_GRACE;
    Some(Captured { status, stdout: stdout.take(deadline), stderr: stderr.take(deadline) })
}

pub fn wait_within(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(SETTLE),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn join_within(readers: impl IntoIterator<Item = JoinHandle<()>>, deadline: Instant) {
    for reader in readers {
        while !reader.is_finished() && Instant::now() < deadline {
            std::thread::sleep(SETTLE);
        }
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
}

struct Collected {
    text: Arc<Mutex<Vec<u8>>>,
    reader: Option<JoinHandle<()>>,
}

impl Collected {
    fn start(stream: Option<impl Read + Send + 'static>) -> Self {
        let text = Arc::new(Mutex::new(Vec::new()));
        let filled = Arc::clone(&text);
        let reader = stream.and_then(|mut stream| {
            std::thread::Builder::new()
                .name("zen-run-capture".to_owned())
                .spawn(move || {
                    let mut chunk = [0_u8; 8192];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(read) => filled
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .extend_from_slice(&chunk[..read]),
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                })
                .ok()
        });
        Self { text, reader }
    }

    fn take(self, deadline: Instant) -> Vec<u8> {
        join_within(self.reader, deadline);
        std::mem::take(&mut *self.text.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_capture_does_not_wait_for_a_grandchild_that_keeps_the_pipes() {
        let started = Instant::now();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 120 & echo $!; echo there >&2"]);
        let captured = capture(&mut command, Duration::from_secs(60)).unwrap();
        let elapsed = started.elapsed();
        let grandchild = String::from_utf8_lossy(&captured.stdout).trim().to_owned();
        let _ = Command::new("/bin/kill").args(["-9", &grandchild]).status();
        assert!(elapsed < Duration::from_secs(30), "{elapsed:?}");
        assert!(captured.status.success());
        assert!(grandchild.parse::<u32>().is_ok(), "{grandchild:?}");
        assert_eq!(captured.stderr, b"there\n");
        let mut slow = Command::new("/bin/sh");
        slow.args(["-c", "sleep 600"]);
        assert!(capture(&mut slow, Duration::from_millis(100)).is_err());
    }
}
