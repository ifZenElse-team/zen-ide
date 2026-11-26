use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zen_releases::publish::{Patience, upload_bytes_with};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Put {
    Slow,
    StopReading,
    Drop,
    Stall,
    StoreThenDrop,
    Ok,
    Refuse,
}

#[derive(Default)]
struct Stub {
    plan: Vec<Put>,
    puts: usize,
    gets: usize,
    stored: Option<(usize, String)>,
}

fn head(stream: &mut TcpStream) -> Option<(String, usize)> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        bytes.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let length = text
        .lines()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_owned()))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    Some((text.split(' ').next().unwrap_or_default().to_owned(), length))
}

fn body(stream: &mut TcpStream, length: usize) -> Vec<u8> {
    let mut body = vec![0; length];
    let _ = stream.read_exact(&mut body);
    body
}

fn answer(stream: &mut TcpStream, status: &str, text: &str) {
    let reply = format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len());
    let _ = stream.write_all(reply.as_bytes());
}

fn serve(stub: Arc<Mutex<Stub>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let stub = Arc::clone(&stub);
            std::thread::spawn(move || {
                let Some((method, length)) = head(&mut stream) else { return };
                if method == "GET" {
                    let stored = {
                        let mut stub = stub.lock().unwrap();
                        stub.gets += 1;
                        stub.stored.clone()
                    };
                    match stored {
                        Some((size, sha)) => {
                            answer(&mut stream, "200 OK", &format!(r#"{{"size":{size},"sha256":"{sha}"}}"#))
                        }
                        None => answer(&mut stream, "404 Not Found", ""),
                    }
                    return;
                }
                let step = {
                    let mut stub = stub.lock().unwrap();
                    stub.puts += 1;
                    let index = stub.puts - 1;
                    stub.plan.get(index).copied().unwrap_or(Put::Ok)
                };
                match step {
                    Put::Slow => {
                        let mut received = Vec::with_capacity(length);
                        let mut chunk = vec![0; 10 * 1024];
                        while received.len() < length {
                            let want = chunk.len().min(length - received.len());
                            let Ok(read) = stream.read(&mut chunk[..want]) else { return };
                            if read == 0 {
                                return;
                            }
                            received.extend_from_slice(&chunk[..read]);
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        stub.lock().unwrap().stored = Some((received.len(), zen_update::sha256_hex(&received)));
                        answer(&mut stream, "201 Created", "");
                    }
                    Put::Drop => {
                        let mut some = vec![0; 64 * 1024];
                        let _ = stream.read(&mut some);
                        drop(stream);
                    }
                    Put::StopReading => {
                        let mut some = vec![0; 64 * 1024];
                        let _ = stream.read(&mut some);
                        std::thread::sleep(Duration::from_secs(30));
                    }
                    Put::Stall => {
                        let _ = body(&mut stream, length);
                        std::thread::sleep(Duration::from_secs(5));
                    }
                    Put::StoreThenDrop | Put::Ok => {
                        let received = body(&mut stream, length);
                        stub.lock().unwrap().stored = Some((received.len(), zen_update::sha256_hex(&received)));
                        if step == Put::Ok {
                            answer(&mut stream, "201 Created", "");
                        } else {
                            let _ = stream.shutdown(std::net::Shutdown::Both);
                        }
                    }
                    Put::Refuse => {
                        let _ = body(&mut stream, length);
                        answer(&mut stream, "403 Forbidden", "no token");
                    }
                }
            });
        }
    });
    base
}

fn patience() -> Patience {
    Patience {
        retries: 3,
        pause: Duration::from_millis(10),
        connect: Duration::from_secs(2),
        answer: Duration::from_millis(700),
        least_rate: u64::MAX,
        send_floor: Duration::from_secs(10),
    }
}

fn file() -> Vec<u8> {
    (0..4 * 1024 * 1024).map(|index| (index % 251) as u8).collect()
}

fn upload(plan: &[Put], stored: Option<(usize, String)>) -> (Result<bool, String>, Arc<Mutex<Stub>>) {
    let stub = Arc::new(Mutex::new(Stub { plan: plan.to_vec(), stored, ..Stub::default() }));
    let base = serve(Arc::clone(&stub));
    let bytes = file();
    (upload_bytes_with(&base, "token", "gogarden", "26.11.9", "gogarden.tar.gz", &bytes, &patience()), stub)
}

#[test]
fn a_dropped_connection_is_tried_again_until_the_file_is_up() {
    let (result, stub) = upload(&[Put::Drop, Put::Drop], None);
    assert_eq!(result, Ok(true));
    let stub = stub.lock().unwrap();
    assert_eq!(stub.puts, 3);
    assert_eq!(stub.stored.as_ref().map(|(size, _)| *size), Some(file().len()));
}

#[test]
fn a_server_that_never_answers_times_out_and_is_tried_again() {
    let started = std::time::Instant::now();
    let (result, stub) = upload(&[Put::Stall], None);
    assert_eq!(result, Ok(true));
    assert_eq!(stub.lock().unwrap().puts, 2);
    assert!(started.elapsed() < Duration::from_secs(5), "the stall did not hold the upload: {:?}", started.elapsed());
}

#[test]
fn three_retries_then_the_upload_fails_with_the_reason() {
    let (result, stub) = upload(&[Put::Drop; 6], None);
    let error = result.unwrap_err();
    assert!(error.contains("after 4 tries"), "{error}");
    assert_eq!(stub.lock().unwrap().puts, 4);
}

#[test]
fn a_file_the_server_already_has_is_not_sent_again() {
    let bytes = file();
    let (result, stub) = upload(&[], Some((bytes.len(), zen_update::sha256_hex(&bytes))));
    assert_eq!(result, Ok(false));
    assert_eq!(stub.lock().unwrap().puts, 0);
    let (result, stub) = upload(&[Put::StoreThenDrop], None);
    assert_eq!(result, Ok(false), "the server kept it before the answer was lost: it is not sent twice");
    assert_eq!(stub.lock().unwrap().puts, 1);
    let (result, stub) = upload(&[], Some((bytes.len(), "0".repeat(64))));
    assert_eq!(result, Ok(true), "another file under the same name is replaced");
    assert_eq!(stub.lock().unwrap().puts, 1);
}

#[test]
fn a_refusal_is_not_tried_again() {
    let (result, stub) = upload(&[Put::Refuse], None);
    assert!(result.unwrap_err().contains("403"));
    assert_eq!(stub.lock().unwrap().puts, 1);
}

fn upload_with(plan: &[Put], bytes: &[u8], patience: &Patience) -> (Result<bool, String>, Arc<Mutex<Stub>>) {
    let stub = Arc::new(Mutex::new(Stub { plan: plan.to_vec(), ..Stub::default() }));
    let base = serve(Arc::clone(&stub));
    (upload_bytes_with(&base, "token", "devopsdaruma", "26.11.9", "big.tar.gz", bytes, patience), stub)
}

#[test]
fn a_slow_but_steady_server_gets_the_whole_file() {
    let bytes: Vec<u8> = (0..2 * 1024 * 1024).map(|index| (index % 253) as u8).collect();
    let steady = Patience {
        least_rate: 16 * 1024,
        send_floor: Duration::from_secs(2),
        answer: Duration::from_secs(2),
        retries: 0,
        ..patience()
    };
    let (result, stub) = upload_with(&[Put::Slow], &bytes, &steady);
    assert!(
        steady.for_answer(bytes.len()) >= Duration::from_secs(120),
        "the answer may wait for what the socket still held"
    );
    assert_eq!(result, Ok(true), "a server that takes about 200 KB/s keeps receiving");
    assert_eq!(stub.lock().unwrap().stored.as_ref().map(|(size, _)| *size), Some(bytes.len()));
}

#[test]
fn a_server_that_stops_reading_fails_the_upload() {
    let bytes: Vec<u8> = (0..8 * 1024 * 1024).map(|index| (index % 251) as u8).collect();
    let stalled =
        Patience { least_rate: 4 * 1024 * 1024, send_floor: Duration::from_secs(1), retries: 0, ..patience() };
    let started = std::time::Instant::now();
    let (result, _) = upload_with(&[Put::StopReading], &bytes, &stalled);
    assert!(result.unwrap_err().contains("cannot upload"));
    assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
}

#[test]
fn the_defaults_allow_a_slow_line_and_the_environment_can_change_them() {
    let patience = Patience::default();
    assert_eq!((patience.least_rate, patience.send_floor), (16 * 1024, Duration::from_secs(600)));
    assert_eq!(patience.for_answer(1024), patience.answer, "a small file waits the plain answer time");
    let big = patience.for_answer(73 * 1024 * 1024);
    assert_eq!(big, patience.answer + Duration::from_secs(1024), "after 73 MB it may wait for 16 MiB more at 16 KiB/s");
    assert_eq!(zen_releases::publish::LARGE_FILE, 20 * 1024 * 1024);
    assert_eq!(
        zen_releases::publish::parallel(3),
        std::env::var(zen_releases::publish::PARALLEL_ENV).ok().and_then(|v| v.parse().ok()).unwrap_or(3)
    );
}
