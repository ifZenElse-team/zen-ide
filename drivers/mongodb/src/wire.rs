use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bson::{Bson, Document};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use zen_db_driver_kit::Failure;

const OP_MSG: i32 = 2013;
const HEADER: usize = 16;
const MORE_TO_COME: u32 = 1 << 1;
const CHECKSUM_PRESENT: u32 = 1;
pub(crate) const MAX_MESSAGE: usize = 48_000_000;

pub(crate) enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(io) => Pin::new(io).poll_read(cx, buf),
            Stream::Tls(io) => Pin::new(io.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(io) => Pin::new(io).poll_write(cx, buf),
            Stream::Tls(io) => Pin::new(io.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(io) => Pin::new(io).poll_flush(cx),
            Stream::Tls(io) => Pin::new(io.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(io) => Pin::new(io).poll_shutdown(cx),
            Stream::Tls(io) => Pin::new(io.as_mut()).poll_shutdown(cx),
        }
    }
}

#[derive(Debug)]
pub(crate) enum WireError {
    Lost(String),
    Server { code: i32, name: String, message: String },
}

impl WireError {
    pub(crate) fn failure(&self) -> Failure {
        match self {
            WireError::Lost(message) => Failure::database("notConnected", message.clone()),
            WireError::Server { code, name, message } => {
                let kind = match *code {
                    18 | 11 => "authentication",
                    13 => "refused",
                    50 => "timeout",
                    11601 => "cancelled",
                    _ => "",
                };
                let text = if name.is_empty() { message.clone() } else { format!("{message} ({name})") };
                if kind.is_empty() {
                    Failure::server(text, Some(code.to_string()), None)
                } else {
                    let mut failure = Failure::database(kind, text);
                    if let Some(data) = failure.data.as_mut() {
                        data["code"] = serde_json::json!(code.to_string());
                    }
                    failure
                }
            }
        }
    }

    pub(crate) fn lost(&self) -> bool {
        matches!(self, WireError::Lost(_))
    }
}

pub(crate) struct Wire<S = Stream> {
    stream: S,
    next_id: i32,
    pub(crate) max_message: usize,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Wire<S> {
    pub(crate) fn new(stream: S) -> Self {
        Self { stream, next_id: 1, max_message: MAX_MESSAGE }
    }

    pub(crate) async fn command(&mut self, database: &str, mut command: Document) -> Result<Document, WireError> {
        command.insert("$db", database);
        let id = self.send(&command).await?;
        let reply = self.receive(id).await?;
        check(reply)
    }

    async fn send(&mut self, command: &Document) -> Result<i32, WireError> {
        let body =
            command.to_vec().map_err(|error| WireError::Lost(format!("the command cannot be encoded: {error}")))?;
        let length = HEADER + 4 + 1 + body.len();
        if length > self.max_message {
            return Err(WireError::Server {
                code: 10334,
                name: "BSONObjectTooLarge".into(),
                message: format!("the command is {length} bytes, more than the server takes"),
            });
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut message = Vec::with_capacity(length);
        message.extend_from_slice(&i32::try_from(length).unwrap_or(i32::MAX).to_le_bytes());
        message.extend_from_slice(&id.to_le_bytes());
        message.extend_from_slice(&0i32.to_le_bytes());
        message.extend_from_slice(&OP_MSG.to_le_bytes());
        message.extend_from_slice(&0u32.to_le_bytes());
        message.push(0);
        message.extend_from_slice(&body);
        self.stream.write_all(&message).await.map_err(lost)?;
        self.stream.flush().await.map_err(lost)?;
        Ok(id)
    }

    async fn receive(&mut self, id: i32) -> Result<Document, WireError> {
        loop {
            let mut header = [0u8; HEADER];
            self.stream.read_exact(&mut header).await.map_err(lost)?;
            let number = |at: usize| i32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]);
            let length = usize::try_from(number(0)).unwrap_or(0);
            if length < HEADER + 5 || length > self.max_message.max(MAX_MESSAGE) {
                return Err(WireError::Lost(format!("the server sent a message of {length} bytes")));
            }
            let mut rest = vec![0u8; length - HEADER];
            self.stream.read_exact(&mut rest).await.map_err(lost)?;
            if number(12) != OP_MSG {
                return Err(WireError::Lost(format!("the server answered with operation {}", number(12))));
            }
            let reply = body(&rest)?;
            if number(8) == id {
                return Ok(reply);
            }
        }
    }
}

fn lost(error: io::Error) -> WireError {
    WireError::Lost(format!("the connection to the server was lost: {error}"))
}

pub(crate) fn body(message: &[u8]) -> Result<Document, WireError> {
    let broken = |what: &str| WireError::Lost(format!("the server's reply is broken: {what}"));
    let flags = u32::from_le_bytes(message.get(..4).ok_or_else(|| broken("no flags"))?.try_into().unwrap_or_default());
    if flags & MORE_TO_COME != 0 {
        return Err(broken("an exhaust reply that was not asked for"));
    }
    let end = if flags & CHECKSUM_PRESENT != 0 { message.len().saturating_sub(4) } else { message.len() };
    let mut at = 4;
    let mut found = None;
    while at < end {
        let kind = message[at];
        at += 1;
        let size = message
            .get(at..at + 4)
            .map(|bytes| usize::try_from(i32::from_le_bytes(bytes.try_into().unwrap_or_default())).unwrap_or(0))
            .ok_or_else(|| broken("a section is cut short"))?;
        let section = message.get(at..at + size).ok_or_else(|| broken("a section is cut short"))?;
        if kind == 0 {
            found = Some(Document::from_reader(section).map_err(|error| broken(&error.to_string()))?);
        }
        at += size.max(1);
    }
    found.ok_or_else(|| broken("no body"))
}

pub(crate) fn check(reply: Document) -> Result<Document, WireError> {
    let ok = match reply.get("ok") {
        Some(Bson::Double(value)) => *value == 1.0,
        Some(Bson::Int32(value)) => *value == 1,
        Some(Bson::Int64(value)) => *value == 1,
        Some(Bson::Boolean(value)) => *value,
        _ => false,
    };
    if ok {
        if let Ok(error) = reply.get_document("writeConcernError") {
            return Err(server_error(error));
        }
        if let Some(Bson::Array(errors)) = reply.get("writeErrors")
            && let Some(Bson::Document(first)) = errors.first()
        {
            return Err(server_error(first));
        }
        return Ok(reply);
    }
    Err(server_error(&reply))
}

fn server_error(reply: &Document) -> WireError {
    let code = match reply.get("code") {
        Some(Bson::Int32(code)) => *code,
        Some(Bson::Int64(code)) => i32::try_from(*code).unwrap_or(0),
        Some(Bson::Double(code)) => *code as i32,
        _ => 0,
    };
    WireError::Server {
        code,
        name: reply.get_str("codeName").unwrap_or_default().to_owned(),
        message: reply.get_str("errmsg").unwrap_or("the server refused the command").to_owned(),
    }
}

#[cfg(test)]
mod tests;
