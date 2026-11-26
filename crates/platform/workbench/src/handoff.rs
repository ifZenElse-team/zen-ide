use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LONGEST_REQUEST: usize = 64 * 1024;

pub const MOST_PATHS: usize = 256;

const SOCKET: &str = "open.sock";

const LONGEST_SOCKET_PATH: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub open: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAnswer {
    pub accepted: bool,
}

pub fn socket_path(data: &Path) -> PathBuf {
    let data = std::path::absolute(data).unwrap_or_else(|_| data.to_owned());
    let preferred = data.join("ipc").join(SOCKET);
    if preferred.as_os_str().len() <= LONGEST_SOCKET_PATH {
        return preferred;
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(data.as_os_str().as_encoded_bytes());
    std::env::temp_dir().join(format!("zen-{}", &hex::encode(digest)[..16])).join(SOCKET)
}

fn request(paths: &[PathBuf], wait: bool) -> OpenRequest {
    OpenRequest {
        open: paths.iter().map(|path| std::path::absolute(path).unwrap_or_else(|_| path.clone())).collect(),
        wait,
    }
}

pub fn send(data: &Path, paths: &[PathBuf]) -> bool {
    imp::send(data, &request(paths, false))
}

pub fn send_and_wait(data: &Path, paths: &[PathBuf]) -> bool {
    imp::send(data, &request(paths, true))
}

#[derive(Debug)]
pub struct Waiting {
    _release: std::sync::mpsc::Sender<()>,
}

#[cfg(unix)]
fn parse_request(line: &[u8]) -> Option<OpenRequest> {
    let request: OpenRequest = serde_json::from_slice(line).ok()?;
    (request.open.len() <= MOST_PATHS && request.open.iter().all(|path| path.is_absolute())).then_some(request)
}

pub use imp::Listener;

#[cfg(unix)]
mod imp {
    use std::io::{Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{LONGEST_REQUEST, OpenAnswer, OpenRequest, Waiting, parse_request, socket_path};

    const WAIT: Duration = Duration::from_secs(5);

    const ANSWER_WAIT: Duration = Duration::from_secs(20);

    const MOST_CLIENTS: usize = 64;

    fn me() -> u32 {
        rustix::process::geteuid().as_raw()
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn same_user(stream: &UnixStream) -> bool {
        rustix::net::sockopt::socket_peercred(stream).is_ok_and(|peer| peer.uid.as_raw() == me())
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn same_user(_: &UnixStream) -> bool {
        true
    }

    fn private_folder(folder: &Path, create: bool) -> std::io::Result<()> {
        if create {
            match std::fs::DirBuilder::new().mode(0o700).create(folder) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let meta = std::fs::symlink_metadata(folder)?;
        if !meta.is_dir() || meta.uid() != me() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not a private folder"));
        }
        if meta.mode() & 0o077 != 0 {
            if !create {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not a private folder"));
            }
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    fn time_out(stream: &UnixStream, wait: Duration) -> bool {
        stream.set_read_timeout(Some(wait)).and_then(|()| stream.set_write_timeout(Some(WAIT))).is_ok()
    }

    fn read_line(mut stream: &UnixStream, longest: usize, wait: Duration) -> Option<Vec<u8>> {
        let deadline = Instant::now() + wait;
        let mut line = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let left = deadline.checked_duration_since(Instant::now()).filter(|left| !left.is_zero())?;
            let _ = stream.set_read_timeout(Some(left));
            let read = match stream.read(&mut buffer) {
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return None,
            };
            if read == 0 {
                return (line.len() <= longest).then_some(line);
            }
            line.extend_from_slice(&buffer[..read]);
            if let Some(end) = line.iter().position(|byte| *byte == b'\n') {
                line.truncate(end);
                return (line.len() <= longest).then_some(line);
            }
            if line.len() > longest {
                return None;
            }
        }
    }

    pub(super) fn send(data: &Path, request: &OpenRequest) -> bool {
        let path = socket_path(data);
        let Some(folder) = path.parent() else { return false };
        if private_folder(folder, false).is_err() {
            return false;
        }
        let Ok(mut stream) = UnixStream::connect(&path) else { return false };
        if !same_user(&stream) || !time_out(&stream, ANSWER_WAIT) {
            return false;
        }
        let Ok(mut line) = serde_json::to_vec(request) else { return false };
        line.push(b'\n');
        if stream.write_all(&line).and_then(|()| stream.flush()).is_err() {
            return false;
        }
        let accepted = read_line(&stream, 1024, ANSWER_WAIT)
            .and_then(|answer| serde_json::from_slice::<OpenAnswer>(&answer).ok())
            .is_some_and(|answer| answer.accepted);
        if accepted && request.wait {
            wait_for_close(&stream);
        }
        accepted
    }

    fn wait_for_close(mut stream: &UnixStream) {
        let _ = stream.set_read_timeout(None);
        let mut buffer = [0u8; 64];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => return,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }

    #[derive(Debug)]
    pub struct Listener {
        path: PathBuf,
        stopped: Arc<AtomicBool>,
        inode: u64,
    }

    impl Listener {
        pub fn start(
            data: &Path,
            accept: impl Fn(Vec<PathBuf>) -> bool + Send + Sync + 'static,
        ) -> std::io::Result<Self> {
            Self::start_waiting(data, move |paths, _| accept(paths))
        }

        pub fn start_waiting(
            data: &Path,
            accept: impl Fn(Vec<PathBuf>, Option<Waiting>) -> bool + Send + Sync + 'static,
        ) -> std::io::Result<Self> {
            std::fs::create_dir_all(data)?;
            let path = socket_path(data);
            let folder = path.parent().ok_or_else(|| std::io::Error::other("no folder for the socket"))?;
            private_folder(folder, true)?;
            let listener = match UnixListener::bind(&path) {
                Ok(listener) => listener,
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    if UnixStream::connect(&path).is_ok() {
                        return Err(error);
                    }
                    std::fs::remove_file(&path)?;
                    UnixListener::bind(&path)?
                }
                Err(error) => return Err(error),
            };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            let inode = std::fs::symlink_metadata(&path)?.ino();
            let stopped = Arc::new(AtomicBool::new(false));
            let stop = Arc::clone(&stopped);
            let accept = Arc::new(accept);
            let clients = Arc::new(AtomicUsize::new(0));
            std::thread::Builder::new().name("zen-handoff".to_owned()).spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    if clients.fetch_add(1, Ordering::SeqCst) >= MOST_CLIENTS {
                        clients.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let (accept, served) = (Arc::clone(&accept), Arc::clone(&clients));
                    let spawned = std::thread::Builder::new().name("zen-handoff-client".to_owned()).spawn(move || {
                        serve(&stream, &*accept);
                        served.fetch_sub(1, Ordering::SeqCst);
                    });
                    if spawned.is_err() {
                        clients.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            })?;
            Ok(Self { path, stopped, inode })
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
            let _ = UnixStream::connect(&self.path);
            if std::fs::symlink_metadata(&self.path).is_ok_and(|meta| meta.ino() == self.inode) {
                let _ = std::fs::remove_file(&self.path);
                if let Some(folder) = self.path.parent() {
                    let _ = std::fs::remove_dir(folder);
                }
            }
        }
    }

    fn serve(stream: &UnixStream, accept: &dyn Fn(Vec<PathBuf>, Option<Waiting>) -> bool) {
        let (release, released) = std::sync::mpsc::channel();
        let mut held = false;
        let accepted = same_user(stream)
            && time_out(stream, WAIT)
            && read_line(stream, LONGEST_REQUEST, WAIT).and_then(|line| parse_request(&line)).is_some_and(|request| {
                held = request.wait;
                accept(request.open, request.wait.then_some(Waiting { _release: release }))
            });
        if let Ok(mut answer) = serde_json::to_vec(&OpenAnswer { accepted }) {
            answer.push(b'\n');
            let _ = (&*stream).write_all(&answer);
        }
        if accepted && held {
            let _: Result<(), _> = released.recv();
        }
    }

    #[cfg(test)]
    mod tests {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        use super::{read_line, time_out};

        #[test]
        fn an_answer_is_read_after_the_peer_has_closed() {
            let (mine, mut peer) = UnixStream::pair().unwrap();
            assert!(time_out(&mine, Duration::from_secs(60)));
            peer.write_all(b"{\"accepted\":true}\n").unwrap();
            drop(peer);
            assert_eq!(read_line(&mine, 1024, Duration::from_secs(60)).as_deref(), Some(&b"{\"accepted\":true}"[..]));
        }

        #[test]
        fn a_silent_peer_is_waited_for_only_so_long() {
            let (mine, _peer) = UnixStream::pair().unwrap();
            assert!(time_out(&mine, Duration::from_secs(60)));
            assert_eq!(read_line(&mine, 1024, Duration::from_millis(50)), None);
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{OpenRequest, Waiting};

    pub(super) fn send(_: &Path, _: &OpenRequest) -> bool {
        false
    }

    #[derive(Debug)]
    pub struct Listener {
        path: PathBuf,
    }

    impl Listener {
        pub fn start(_: &Path, _: impl Fn(Vec<PathBuf>) -> bool + Send + Sync + 'static) -> std::io::Result<Self> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }

        pub fn start_waiting(
            _: &Path,
            _: impl Fn(Vec<PathBuf>, Option<Waiting>) -> bool + Send + Sync + 'static,
        ) -> std::io::Result<Self> {
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    use super::*;

    fn listen(data: &Path) -> (Listener, Receiver<Vec<PathBuf>>) {
        let (sender, received) = channel();
        let listener = Listener::start(data, move |paths| sender.send(paths).is_ok()).unwrap();
        (listener, received)
    }

    #[test]
    fn a_second_start_hands_its_paths_to_the_ide_of_its_data_folder() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (listener, received) = listen(&data);
        let file = temp.path().join("main.go");
        std::fs::write(&file, "package main\n").unwrap();
        let asked = vec![PathBuf::from(format!("{}:12:5", file.display())), temp.path().to_owned()];
        assert!(send(&data, &asked));
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), asked);
        assert!(send(&data, &[]), "no paths: the IDE comes to the front");
        assert_eq!(received.recv_timeout(Duration::from_secs(5)).unwrap(), Vec::<PathBuf>::new());
        drop(listener);
        assert!(!socket_path(&data).exists(), "the socket goes with the IDE");
        assert!(!send(&data, &asked), "nobody listens: the start opens a window");
    }

    #[test]
    fn a_waiting_start_returns_when_the_ide_lets_it_go() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (taken, waiting) = channel();
        let _listener = Listener::start_waiting(&data, move |paths, wait| {
            assert!(wait.is_some());
            taken.send((paths, wait)).is_ok()
        })
        .unwrap();
        let (returned, ended) = channel();
        let client = data.clone();
        std::thread::spawn(move || returned.send(send_and_wait(&client, &[PathBuf::from("/tmp/COMMIT_EDITMSG")])));
        let (paths, wait) = waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(paths, [PathBuf::from("/tmp/COMMIT_EDITMSG")]);
        assert!(ended.recv_timeout(Duration::from_millis(100)).is_err(), "it waits while the file is open");
        drop(wait);
        assert!(ended.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    #[test]
    fn a_plain_start_carries_no_wait_field_so_an_older_ide_reads_it_and_a_newer_request_parses() {
        let line = serde_json::to_string(&request(&[PathBuf::from("/a")], false)).unwrap();
        assert_eq!(line, r#"{"open":["/a"]}"#);
        assert!(parse_request(br#"{"open":["/a"],"wait":true}"#).is_some_and(|request| request.wait));
    }

    #[test]
    fn relative_paths_go_over_absolute() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (_listener, received) = listen(&data);
        assert!(send(&data, &[PathBuf::from("main.go:3")]));
        let got = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, vec![std::env::current_dir().unwrap().join("main.go:3")]);
    }

    #[test]
    fn another_data_folder_never_reaches_this_ide() {
        let temp = tempfile::tempdir().unwrap();
        let mine = temp.path().join("owner");
        let other = temp.path().join("agent");
        let (_listener, received) = listen(&mine);
        assert_ne!(socket_path(&mine), socket_path(&other));
        assert!(!send(&other, &[temp.path().to_owned()]), "another data folder finds no IDE");
        assert!(received.recv_timeout(Duration::from_millis(300)).is_err(), "and this IDE hears nothing");
        std::fs::create_dir_all(&other).unwrap();
        let (_second, theirs) = listen(&other);
        assert!(send(&other, &[temp.path().to_owned()]));
        assert!(theirs.recv_timeout(Duration::from_secs(5)).is_ok());
        assert!(received.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn only_the_owner_may_reach_the_socket() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (listener, _received) = listen(&data);
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(listener.path().parent().unwrap()), 0o700);
        assert_eq!(mode(listener.path()), 0o600);
        drop(listener);
        let folder = socket_path(&data).parent().unwrap().to_owned();
        assert!(!folder.exists(), "the folder goes with the IDE");
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o777)).unwrap();
        let (listener, _received) = listen(&data);
        assert_eq!(mode(listener.path().parent().unwrap()), 0o700, "a folder opened to others is closed again");
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!send(&data, &[temp.path().to_owned()]), "a client never talks through a folder others may enter");
    }

    #[test]
    fn a_second_listener_of_the_same_data_folder_is_refused_and_a_stale_socket_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (listener, _received) = listen(&data);
        assert!(Listener::start(&data, |_| true).is_err(), "one IDE per data folder");
        let path = listener.path().to_owned();
        std::mem::forget(listener);
        std::fs::remove_file(&path).unwrap();
        let stale = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
        drop(stale);
        assert!(path.exists(), "a crashed IDE leaves its socket");
        let (_listener, received) = listen(&data);
        assert!(send(&data, &[temp.path().to_owned()]));
        assert!(received.recv_timeout(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn anything_but_an_open_request_is_refused() {
        use std::io::{BufRead, BufReader, Write};
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (listener, received) = listen(&data);
        let ask = |line: &[u8]| {
            let mut stream = std::os::unix::net::UnixStream::connect(listener.path()).unwrap();
            let _ = stream.write_all(line);
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let mut answer = String::new();
            BufReader::new(stream).read_line(&mut answer).unwrap();
            serde_json::from_str::<OpenAnswer>(&answer).unwrap()
        };
        assert!(!ask(b"{\"command\":\"run\"}\n").accepted);
        assert!(!ask(b"{\"open\":[\"relative/path\"]}\n").accepted);
        assert!(!ask(b"not json\n").accepted);
        let many: Vec<String> = (0..=MOST_PATHS).map(|n| format!("/tmp/{n}")).collect();
        assert!(!ask(format!("{}\n", serde_json::json!({ "open": many })).as_bytes()).accepted);
        let huge = format!("{{\"open\":[\"/{}\"]}}\n", "a".repeat(LONGEST_REQUEST));
        assert!(!ask(huge.as_bytes()).accepted);
        assert!(received.recv_timeout(Duration::from_millis(300)).is_err(), "nothing reached the IDE");
        assert!(ask(b"{\"open\":[\"/tmp\"]}\n").accepted);
    }

    #[test]
    fn clients_that_stall_never_hold_up_a_second_start() {
        use std::io::{BufRead, BufReader, Write};
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let (listener, received) = listen(&data);
        let idle: Vec<_> = (0..10).map(|_| std::os::unix::net::UnixStream::connect(listener.path()).unwrap()).collect();
        let trickle = std::os::unix::net::UnixStream::connect(listener.path()).unwrap();
        (&trickle).write_all(b"{\"open\":[").unwrap();
        assert!(send(&data, &[temp.path().to_owned()]));
        assert!(received.recv_timeout(Duration::from_secs(60)).is_ok());
        let writer = trickle.try_clone().unwrap();
        let answered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = std::sync::Arc::clone(&answered);
        let trickling = std::thread::spawn(move || {
            let mut writer = writer;
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
                if writer.write_all(b" ").is_err() {
                    break;
                }
            }
        });
        trickle.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
        let mut answer = String::new();
        BufReader::new(&trickle).read_line(&mut answer).unwrap();
        answered.store(true, std::sync::atomic::Ordering::SeqCst);
        trickling.join().unwrap();
        assert!(!serde_json::from_str::<OpenAnswer>(&answer).unwrap().accepted, "{answer}");
        drop(idle);
    }

    #[test]
    fn a_folder_that_is_a_symbolic_link_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let elsewhere = temp.path().join("elsewhere");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o700)).unwrap();
        let folder = socket_path(&data).parent().unwrap().to_owned();
        std::os::unix::fs::symlink(&elsewhere, &folder).unwrap();
        assert!(Listener::start(&data, |_| true).is_err(), "the IDE never listens through a link");
        let (theirs, received) = listen(&temp.path().join("real"));
        std::os::unix::fs::symlink(theirs.path(), elsewhere.join(SOCKET)).unwrap();
        assert!(!send(&data, &[temp.path().to_owned()]), "nor does a start talk through one");
        assert!(received.recv_timeout(Duration::from_millis(300)).is_err());
        let long = temp.path().join("d".repeat(120));
        let fallback = socket_path(&long).parent().unwrap().to_owned();
        let _ = std::fs::remove_file(&fallback);
        std::os::unix::fs::symlink(&elsewhere, &fallback).unwrap();
        let refused = Listener::start(&long, |_| true);
        std::fs::remove_file(&fallback).unwrap();
        assert!(refused.is_err(), "a link planted in the temporary folder is refused too");
    }

    #[test]
    fn a_long_data_folder_listens_in_a_private_temporary_folder() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("d".repeat(120));
        let path = socket_path(&data);
        assert!(path.as_os_str().len() < 104, "{}", path.display());
        assert!(path.starts_with(std::env::temp_dir()));
        let (listener, received) = listen(&data);
        assert!(send(&data, &[temp.path().to_owned()]));
        assert!(received.recv_timeout(Duration::from_secs(5)).is_ok());
        let folder = listener.path().parent().unwrap().to_owned();
        drop(listener);
        assert!(!folder.exists(), "the temporary folder goes with the IDE");
    }
}
