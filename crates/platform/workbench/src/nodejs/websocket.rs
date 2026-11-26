use std::io::{BufRead, BufReader, Error, ErrorKind, Read, Result, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use zen_i18n::t;

const MOST_BYTES: u64 = 256 * 1024 * 1024;
const TEXT: u8 = 0x1;
const CLOSE: u8 = 0x8;
const PING: u8 = 0x9;
const PONG: u8 = 0xA;

static MASKS: AtomicU32 = AtomicU32::new(0);

fn mask() -> [u8; 4] {
    let time = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.subsec_nanos());
    time.wrapping_add(MASKS.fetch_add(0x9E37_79B9, Ordering::Relaxed)).to_le_bytes()
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidData, message.to_owned())
}

fn write_frame(stream: &mut TcpStream, code: u8, payload: &[u8]) -> Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | code);
    match payload.len() {
        length @ 0..=125 => frame.push(0x80 | length as u8),
        length @ 126..=65_535 => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        }
        length => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
    }
    let key = mask();
    frame.extend_from_slice(&key);
    frame.extend(payload.iter().enumerate().map(|(at, byte)| byte ^ key[at % 4]));
    stream.write_all(&frame)
}

pub(crate) struct Writer {
    stream: TcpStream,
}

impl Writer {
    pub(crate) fn send(&mut self, text: &str) -> Result<()> {
        write_frame(&mut self.stream, TEXT, text.as_bytes())
    }

    pub(crate) fn close(&mut self) {
        let _ = write_frame(&mut self.stream, CLOSE, &[]);
    }
}

pub(crate) struct Reader {
    stream: BufReader<TcpStream>,
    pong: TcpStream,
}

impl Reader {
    fn frame(&mut self) -> Result<(bool, u8, Vec<u8>)> {
        let mut head = [0; 2];
        self.stream.read_exact(&mut head)?;
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0; 2];
                self.stream.read_exact(&mut bytes)?;
                u64::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0; 8];
                self.stream.read_exact(&mut bytes)?;
                u64::from_be_bytes(bytes)
            }
            length => u64::from(length),
        };
        if length > MOST_BYTES {
            return Err(invalid(&t!("workbench-node-debug-too-large")));
        }
        let mut key = [0; 4];
        if head[1] & 0x80 != 0 {
            self.stream.read_exact(&mut key)?;
        }
        let mut payload = vec![0; length as usize];
        self.stream.read_exact(&mut payload)?;
        payload.iter_mut().enumerate().for_each(|(at, byte)| *byte ^= key[at % 4]);
        Ok((head[0] & 0x80 != 0, head[0] & 0x0F, payload))
    }

    pub(crate) fn next_text(&mut self) -> Result<Option<String>> {
        let mut message = Vec::new();
        loop {
            let (last, code, payload) = self.frame()?;
            match code {
                CLOSE => return Ok(None),
                PING => write_frame(&mut self.pong, PONG, &payload)?,
                PONG => {}
                _ => {
                    message.extend_from_slice(&payload);
                    if last {
                        return String::from_utf8(message)
                            .map(Some)
                            .map_err(|_| invalid(&t!("workbench-node-debug-not-text")));
                    }
                }
            }
        }
    }
}

fn parts(url: &str) -> Result<(&str, &str)> {
    let rest = url.strip_prefix("ws://").ok_or_else(|| invalid(&t!("workbench-node-debug-not-ws")))?;
    Ok(rest.split_once('/').map_or((rest, ""), |(host, path)| (host, path)))
}

pub(crate) fn connect(url: &str) -> Result<(Reader, Writer)> {
    let (host, path) = parts(url)?;
    if !["127.0.0.1:", "localhost:", "[::1]:"].iter().any(|local| host.starts_with(local)) {
        return Err(invalid(&t!("workbench-node-debug-not-local", host = host)));
    }
    let mut stream = TcpStream::connect(host)?;
    stream.set_nodelay(true)?;
    write!(
        stream,
        "GET /{path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )?;
    let writer = stream.try_clone()?;
    let pong = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    if !status.contains(" 101") {
        return Err(invalid(status.trim()));
    }
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(invalid(&t!("workbench-node-debug-closed")));
        }
        if line.trim().is_empty() {
            break;
        }
    }
    Ok((Reader { stream: reader, pong }, Writer { stream: writer }))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    fn read_client_frame(reader: &mut BufReader<TcpStream>) -> (u8, Vec<u8>) {
        let mut head = [0; 2];
        reader.read_exact(&mut head).unwrap();
        assert_ne!(head[1] & 0x80, 0, "a client masks every frame");
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0; 2];
                reader.read_exact(&mut bytes).unwrap();
                usize::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0; 8];
                reader.read_exact(&mut bytes).unwrap();
                u64::from_be_bytes(bytes) as usize
            }
            length => usize::from(length),
        };
        let mut key = [0; 4];
        reader.read_exact(&mut key).unwrap();
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload).unwrap();
        payload.iter_mut().enumerate().for_each(|(at, byte)| *byte ^= key[at % 4]);
        (head[0] & 0x0F, payload)
    }

    #[test]
    fn it_talks_text_in_frames_of_every_size_and_answers_pings() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/session-id", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
            }
            out.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n").unwrap();
            let (_, short) = read_client_frame(&mut reader);
            let (_, long) = read_client_frame(&mut reader);
            out.write_all(&[0x89, 2, b'h', b'i']).unwrap();
            out.write_all(&[0x01, 3, b'o', b'n', b'e']).unwrap();
            out.write_all(&[0x80, 3, b't', b'w', b'o']).unwrap();
            out.write_all(&[0x81, 126, 0x01, 0x00]).unwrap();
            out.write_all(&[b'x'; 256]).unwrap();
            let (code, pong) = read_client_frame(&mut reader);
            out.write_all(&[0x88, 0]).unwrap();
            (request, short, long, code, pong)
        });
        let (mut reader, mut writer) = connect(&url).unwrap();
        writer.send("short").unwrap();
        writer.send(&"y".repeat(70_000)).unwrap();
        assert_eq!(reader.next_text().unwrap().as_deref(), Some("onetwo"), "fragments are one message");
        assert_eq!(reader.next_text().unwrap().unwrap().len(), 256);
        assert_eq!(reader.next_text().unwrap(), None, "a close ends the stream");
        let (request, short, long, code, pong) = server.join().unwrap();
        assert!(request.starts_with("GET /session-id HTTP/1.1"), "{request}");
        assert_eq!(short, b"short");
        assert_eq!(long.len(), 70_000);
        assert_eq!((code, pong.as_slice()), (PONG, &b"hi"[..]));
    }

    #[test]
    fn only_a_ws_address_of_a_server_that_switches_protocols_connects() {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        assert!(connect("ws://evil.example:80/x").err().unwrap().to_string().contains("not on this computer"));
        assert!(connect("http://127.0.0.1:1/x").is_err());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/x", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            out.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").unwrap();
        });
        assert!(connect(&url).is_err());
        server.join().unwrap();
    }
}
