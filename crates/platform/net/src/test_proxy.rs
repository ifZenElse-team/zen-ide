use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

fn read_head(stream: &TcpStream) -> String {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut head = String::new();
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
        head.push_str(&line);
        line.clear();
    }
    head
}

pub const TARGET_HOST: &str = "zen-target.test";

pub fn target() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            read_head(&stream);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

pub struct Proxy {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Proxy {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let seen = Arc::clone(&seen);
                std::thread::spawn(move || tunnel(client, &seen));
            }
        });
        Self { port, requests }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn tunnel(mut client: TcpStream, seen: &Mutex<Vec<String>>) {
    let head = read_head(&client);
    seen.lock().unwrap().push(head.clone());
    let address = head.split_whitespace().nth(1).unwrap_or_default().replace(TARGET_HOST, "127.0.0.1");
    let Ok(mut server) = TcpStream::connect(address) else { return };
    let _ = client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    let (mut up, mut down) = (client.try_clone().unwrap(), server.try_clone().unwrap());
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut up, &mut down);
    });
    let mut buffer = [0u8; 4096];
    while let Ok(read) = server.read(&mut buffer) {
        if read == 0 || client.write_all(&buffer[..read]).is_err() {
            break;
        }
    }
    let _ = client.shutdown(std::net::Shutdown::Both);
}

pub fn tls_target(certificate_pem: &str, key_pem: &str) -> u16 {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    crate::install_tls();
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(certificate_pem.as_bytes()).map(Result::unwrap).collect();
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let config = Arc::clone(&config);
            std::thread::spawn(move || {
                let connection = rustls::ServerConnection::new(config).unwrap();
                let mut tls = rustls::StreamOwned::new(connection, stream);
                let mut request = [0u8; 2048];
                if tls.read(&mut request).is_ok_and(|read| read > 0) {
                    let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
                    tls.conn.send_close_notify();
                    let _ = tls.flush();
                }
            });
        }
    });
    port
}
