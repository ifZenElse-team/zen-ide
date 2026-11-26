use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

const HEADER: usize = 8;
const PACKET: usize = 4096;
const PRELOGIN: u8 = 0x12;
const ENCRYPTION_OPTION: u8 = 0x01;
const ATTENTION: u8 = 0x06;
const DONE: u8 = 0xfd;
const DONE_SIZE: usize = 13;
const DONE_ATTENTION: u8 = 0x20;
const ON: u8 = 1;
const NOT_SUPPORTED: u8 = 2;
const REQUIRED: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wanted {
    Never,
    IfOffered,
    Always,
}

pub(crate) struct Link<S> {
    inner: Inner<S>,
    hello: Hello,
    answers: Answers,
}

enum Inner<S> {
    Strict(Box<TlsStream<S>>),
    Negotiated(Box<Negotiated<S>>),
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Link<S> {
    pub(crate) async fn strict(io: S, config: Arc<ClientConfig>, name: ServerName<'static>) -> io::Result<Self> {
        let stream = TlsConnector::from(config).connect(name, io).await?;
        Ok(Self { inner: Inner::Strict(Box::new(stream)), hello: Hello::new(REQUIRED), answers: Answers::default() })
    }

    pub(crate) fn negotiated(io: S, config: Arc<ClientConfig>, name: ServerName<'static>, wanted: Wanted) -> Self {
        let level = match wanted {
            Wanted::Never => NOT_SUPPORTED,
            Wanted::IfOffered | Wanted::Always => ON,
        };
        let negotiated = Negotiated {
            state: State::Plain(Framed::new(io)),
            connector: TlsConnector::from(config),
            name,
            wanted,
            answer: Answer::default(),
        };
        Self { inner: Inner::Negotiated(Box::new(negotiated)), hello: Hello::new(level), answers: Answers::default() }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Inner<S> {
    fn pinned(&mut self) -> Pin<&mut (dyn AsyncReadWrite + Send)> {
        match self {
            Inner::Strict(io) => Pin::new(io.as_mut()),
            Inner::Negotiated(io) => Pin::new(io.as_mut()),
        }
    }
}

trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin {}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncReadWrite for T {}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncRead for Link<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.answers.poll_read(&mut this.inner, cx, buf)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncWrite for Link<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.hello.collect(buf) {
            return Poll::Ready(Ok(buf.len()));
        }
        ready!(this.hello.send(&mut this.inner, cx))?;
        if buf.first() == Some(&ATTENTION) && buf.len() >= HEADER {
            this.answers.attention();
        }
        this.inner.pinned().poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.hello.send(&mut this.inner, cx))?;
        this.inner.pinned().poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().inner.pinned().poll_shutdown(cx)
    }
}

struct Hello {
    level: u8,
    buffer: Vec<u8>,
    written: usize,
    whole: bool,
    sent: bool,
}

impl Hello {
    fn new(level: u8) -> Self {
        Self { level, buffer: Vec::new(), written: 0, whole: false, sent: false }
    }

    fn collect(&mut self, data: &[u8]) -> bool {
        if self.whole {
            return false;
        }
        self.buffer.extend_from_slice(data);
        let Some(header) = self.buffer.get(..HEADER) else { return true };
        let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
        if header[0] == PRELOGIN && self.buffer.len() < length {
            return true;
        }
        if header[0] == PRELOGIN {
            set_encryption(&mut self.buffer[HEADER..length.max(HEADER)], self.level);
        }
        self.whole = true;
        true
    }

    fn send<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &mut self,
        inner: &mut Inner<S>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.sent || !self.whole {
            return Poll::Ready(Ok(()));
        }
        while self.written < self.buffer.len() {
            let written = ready!(inner.pinned().poll_write(cx, &self.buffer[self.written..]))?;
            if written == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.written += written;
        }
        self.sent = true;
        self.buffer = Vec::new();
        Poll::Ready(Ok(()))
    }
}

#[derive(Default)]
struct Answers {
    attention: bool,
    header: usize,
    left: usize,
    packet: Vec<u8>,
    ready: Vec<u8>,
    given: usize,
    tail: Vec<u8>,
}

impl Answers {
    fn attention(&mut self) {
        self.attention = true;
        self.tail.clear();
    }

    fn poll_read<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &mut self,
        inner: &mut Inner<S>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.given < self.ready.len() {
            let count = (self.ready.len() - self.given).min(buf.remaining());
            buf.put_slice(&self.ready[self.given..self.given + count]);
            self.given += count;
            return Poll::Ready(Ok(()));
        }
        if self.attention && self.header == 0 && self.left == 0 {
            ready!(self.whole_packet(inner, cx))?;
            self.mark();
            self.ready = std::mem::take(&mut self.packet);
            self.given = 0;
            return self.poll_read(inner, cx, buf);
        }
        let limit = match (self.attention, self.header, self.left) {
            (false, _, _) => buf.remaining(),
            (true, 0, left) => left,
            (true, header, _) => HEADER - header,
        };
        let mut scratch = vec![0u8; limit.min(buf.remaining()).min(64 * 1024)];
        let mut part = ReadBuf::new(&mut scratch);
        ready!(inner.pinned().poll_read(cx, &mut part))?;
        buf.put_slice(part.filled());
        self.track(part.filled());
        Poll::Ready(Ok(()))
    }

    fn track(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            if self.left > 0 {
                let count = self.left.min(data.len());
                self.left -= count;
                data = &data[count..];
                continue;
            }
            let count = (HEADER - self.header).min(data.len());
            self.packet.extend_from_slice(&data[..count]);
            self.header += count;
            data = &data[count..];
            if self.header == HEADER {
                self.left = usize::from(u16::from_be_bytes([self.packet[2], self.packet[3]])).saturating_sub(HEADER);
                self.header = 0;
                self.packet.clear();
            }
        }
    }

    fn whole_packet<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &mut self,
        inner: &mut Inner<S>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let wanted = match self.packet.get(..HEADER) {
                Some(header) => usize::from(u16::from_be_bytes([header[2], header[3]])).max(HEADER),
                None => HEADER,
            };
            if self.packet.len() >= wanted {
                return Poll::Ready(Ok(()));
            }
            let mut scratch = vec![0u8; wanted - self.packet.len()];
            let mut part = ReadBuf::new(&mut scratch);
            ready!(inner.pinned().poll_read(cx, &mut part))?;
            if part.filled().is_empty() {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            self.packet.extend_from_slice(part.filled());
        }
    }

    fn mark(&mut self) {
        self.tail.extend_from_slice(&self.packet[HEADER..]);
        let excess = self.tail.len().saturating_sub(DONE_SIZE);
        self.tail.drain(..excess);
        if self.packet[1] & 1 == 0 {
            return;
        }
        let acknowledged = self.tail.len() == DONE_SIZE && self.tail[0] == DONE && self.tail[1] & DONE_ATTENTION != 0;
        if acknowledged {
            self.attention = false;
        } else {
            self.packet[1] &= !1;
        }
        self.tail.clear();
    }
}

fn set_encryption(payload: &mut [u8], level: u8) {
    let mut at = 0;
    while let Some(&token) = payload.get(at) {
        if token == 0xff {
            return;
        }
        let Some(entry) = payload.get(at + 1..at + 5) else { return };
        let offset = usize::from(u16::from_be_bytes([entry[0], entry[1]]));
        let length = usize::from(u16::from_be_bytes([entry[2], entry[3]]));
        if token == ENCRYPTION_OPTION && length >= 1 {
            if let Some(byte) = payload.get_mut(offset) {
                *byte = level;
            }
            return;
        }
        at += 5;
    }
}

#[derive(Default)]
struct Answer {
    bytes: Vec<u8>,
    encryption: Option<u8>,
}

impl Answer {
    fn feed(&mut self, data: &[u8]) {
        if self.encryption.is_some() || self.bytes.len() > 64 * 1024 {
            return;
        }
        self.bytes.extend_from_slice(data);
        let mut payload = Vec::new();
        let mut at = 0;
        loop {
            let Some(header) = self.bytes.get(at..at + HEADER) else { return };
            let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
            if length < HEADER {
                self.encryption = Some(NOT_SUPPORTED);
                return;
            }
            let Some(body) = self.bytes.get(at + HEADER..at + length) else { return };
            payload.extend_from_slice(body);
            at += length;
            if header[1] & 1 == 1 {
                self.encryption = Some(encryption_of(&payload).unwrap_or(NOT_SUPPORTED));
                return;
            }
        }
    }
}

fn encryption_of(payload: &[u8]) -> Option<u8> {
    let mut at = 0;
    while let Some(&token) = payload.get(at) {
        if token == 0xff {
            return None;
        }
        let entry = payload.get(at + 1..at + 5)?;
        let offset = usize::from(u16::from_be_bytes([entry[0], entry[1]]));
        let length = usize::from(u16::from_be_bytes([entry[2], entry[3]]));
        if token == ENCRYPTION_OPTION && length >= 1 {
            return payload.get(offset).map(|level| level & 0x0f);
        }
        at += 5;
    }
    None
}

type Handshake<S> = Pin<Box<dyn Future<Output = io::Result<TlsStream<Framed<S>>>> + Send>>;

enum State<S> {
    Plain(Framed<S>),
    Handshaking(Handshake<S>),
    Tls(Box<TlsStream<Framed<S>>>),
    Failed(String),
    Gone,
}

pub(crate) struct Negotiated<S> {
    state: State<S>,
    connector: TlsConnector,
    name: ServerName<'static>,
    wanted: Wanted,
    answer: Answer,
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> Negotiated<S> {
    fn refused(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::ConnectionRefused, format!("TLS: {message}"))
    }

    fn decide(&mut self) -> io::Result<()> {
        let State::Plain(_) = &self.state else { return Ok(()) };
        let Some(level) = self.answer.encryption else { return Ok(()) };
        let start = match (self.wanted, level) {
            (Wanted::Never, REQUIRED) => {
                self.state =
                    State::Failed("TLS: the server requires encryption: choose a TLS mode other than Disable".into());
                return Err(Self::refused("the server requires encryption"));
            }
            (Wanted::Never, _) | (Wanted::IfOffered, NOT_SUPPORTED) => false,
            (Wanted::Always, NOT_SUPPORTED) => {
                self.state = State::Failed("TLS: the server does not support encryption".into());
                return Err(Self::refused("the server does not support encryption"));
            }
            _ => true,
        };
        if !start {
            self.answer.encryption = Some(NOT_SUPPORTED);
            self.wanted = Wanted::Never;
            return Ok(());
        }
        let State::Plain(mut framed) = std::mem::replace(&mut self.state, State::Gone) else { return Ok(()) };
        framed.wrapping = true;
        let connect = self.connector.connect(self.name.clone(), framed);
        self.state = State::Handshaking(Box::pin(connect));
        Ok(())
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.decide()?;
        loop {
            match &mut self.state {
                State::Handshaking(handshake) => match ready!(handshake.as_mut().poll(cx)) {
                    Ok(mut stream) => {
                        stream.get_mut().0.wrapping = false;
                        self.state = State::Tls(Box::new(stream));
                    }
                    Err(error) => {
                        self.state = State::Failed(format!("TLS: {error}"));
                        return Poll::Ready(Err(io::Error::new(error.kind(), format!("TLS: {error}"))));
                    }
                },
                State::Failed(message) => return Poll::Ready(Err(io::Error::other(message.clone()))),
                State::Gone => return Poll::Ready(Err(io::Error::other("the connection is gone"))),
                State::Plain(_) | State::Tls(_) => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncRead for Negotiated<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let State::Handshaking(_) = this.state {
            ready!(this.poll_ready(cx))?;
        }
        match &mut this.state {
            State::Plain(framed) => {
                let before = buf.filled().len();
                ready!(Pin::new(framed).poll_read(cx, buf))?;
                this.answer.feed(&buf.filled()[before..]);
                Poll::Ready(Ok(()))
            }
            State::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
            State::Failed(message) => Poll::Ready(Err(io::Error::other(message.clone()))),
            State::Handshaking(_) | State::Gone => Poll::Ready(Err(io::Error::other("the connection is gone"))),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncWrite for Negotiated<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.poll_ready(cx))?;
        match &mut this.state {
            State::Plain(framed) => Pin::new(framed).poll_write(cx, buf),
            State::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
            _ => Poll::Ready(Err(io::Error::other("the connection is gone"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.poll_ready(cx))?;
        match &mut this.state {
            State::Plain(framed) => Pin::new(framed).poll_flush(cx),
            State::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
            _ => Poll::Ready(Err(io::Error::other("the connection is gone"))),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().state {
            State::Plain(framed) => Pin::new(framed).poll_shutdown(cx),
            State::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
            _ => Poll::Ready(Ok(())),
        }
    }
}

pub(crate) struct Framed<S> {
    io: S,
    wrapping: bool,
    outgoing: Vec<u8>,
    sending: Vec<u8>,
    sent: usize,
    packet_id: u8,
    header: [u8; HEADER],
    header_read: usize,
    payload_left: usize,
}

impl<S> Framed<S> {
    pub(crate) fn new(io: S) -> Self {
        Self {
            io,
            wrapping: false,
            outgoing: Vec::new(),
            sending: Vec::new(),
            sent: 0,
            packet_id: 1,
            header: [0; HEADER],
            header_read: 0,
            payload_left: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn wrap(&mut self, wrapping: bool) {
        self.wrapping = wrapping;
    }

    fn packets(&mut self) {
        let chunks: Vec<Vec<u8>> = self.outgoing.chunks(PACKET - HEADER).map(<[u8]>::to_vec).collect();
        let last = chunks.len().saturating_sub(1);
        for (index, chunk) in chunks.into_iter().enumerate() {
            let length = u16::try_from(chunk.len() + HEADER).unwrap_or(u16::MAX).to_be_bytes();
            let status = u8::from(index == last);
            self.sending.extend_from_slice(&[PRELOGIN, status, length[0], length[1], 0, 0, self.packet_id, 0]);
            self.sending.extend_from_slice(&chunk);
            self.packet_id = self.packet_id.wrapping_add(1);
        }
        self.outgoing.clear();
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Framed<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !this.wrapping {
            return Pin::new(&mut this.io).poll_read(cx, buf);
        }
        loop {
            if this.payload_left > 0 {
                let mut scratch = [0u8; PACKET];
                let wanted = this.payload_left.min(buf.remaining()).min(PACKET);
                let mut part = ReadBuf::new(&mut scratch[..wanted]);
                ready!(Pin::new(&mut this.io).poll_read(cx, &mut part))?;
                let read = part.filled().len();
                this.payload_left -= read;
                buf.put_slice(part.filled());
                return Poll::Ready(Ok(()));
            }
            let mut part = ReadBuf::new(&mut this.header[this.header_read..]);
            ready!(Pin::new(&mut this.io).poll_read(cx, &mut part))?;
            let read = part.filled().len();
            if read == 0 {
                return Poll::Ready(Ok(()));
            }
            this.header_read += read;
            if this.header_read == HEADER {
                this.header_read = 0;
                let length = usize::from(u16::from_be_bytes([this.header[2], this.header[3]]));
                this.payload_left = length.saturating_sub(HEADER);
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Framed<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if !this.wrapping {
            return Pin::new(&mut this.io).poll_write(cx, buf);
        }
        this.outgoing.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !this.outgoing.is_empty() && this.sending.is_empty() {
            this.packets();
        }
        while this.sent < this.sending.len() {
            let written = ready!(Pin::new(&mut this.io).poll_write(cx, &this.sending[this.sent..]))?;
            if written == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            this.sent += written;
        }
        this.sending.clear();
        this.sent = 0;
        Pin::new(&mut this.io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
