use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Received {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

pub struct TestServer {
    port: u16,
    received: Arc<Mutex<Vec<Received>>>,
    connections: Arc<AtomicUsize>,
}

impl TestServer {
    pub fn start(replies: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let port = listener.local_addr().expect("the local address").port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let (seen, count) = (Arc::clone(&received), Arc::clone(&connections));
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else { return };
                count.fetch_add(1, Ordering::SeqCst);
                serve(stream, &reply, &seen);
            }
        });
        Self { port, received, connections }
    }

    pub fn reply(status: u16, reason: &str, headers: &[(&str, &str)], body: &str) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
        for (name, value) in headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        out.push_str("\r\n");
        out.push_str(body);
        out.into_bytes()
    }

    pub fn json(status: u16, body: &str) -> Vec<u8> {
        Self::reply(status, if status == 200 { "OK" } else { "Status" }, &[("Content-Type", "application/json")], body)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn finish(&self) -> Vec<Received> {
        self.received.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

fn serve(stream: TcpStream, reply: &[u8], seen: &Mutex<Vec<Received>>) -> Option<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut words = line.split_whitespace();
    let method = words.next()?.to_owned();
    let target = words.next()?.to_owned();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let length = headers.iter().find(|(name, _)| name == "content-length").and_then(|(_, value)| value.parse().ok());
    let mut body = vec![0; length.unwrap_or(0)];
    reader.read_exact(&mut body).ok()?;
    seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(Received { method, target, headers, body });
    let mut stream = stream;
    stream.write_all(reply).ok()?;
    stream.flush().ok()
}
