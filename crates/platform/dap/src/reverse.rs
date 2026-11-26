use std::io::{Error, ErrorKind};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct ReverseListener {
    listener: TcpListener,
    addr: SocketAddr,
}

impl ReverseListener {
    pub fn bind() -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener, addr })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn accept(self, timeout: Duration) -> std::io::Result<TcpStream> {
        self.accept_while(timeout, || true)
    }

    pub fn accept_while(self, timeout: Duration, waiting: impl Fn() -> bool) -> std::io::Result<TcpStream> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, peer)) if peer.ip().is_loopback() => {
                    stream.set_nonblocking(false)?;
                    stream.set_nodelay(true)?;
                    return Ok(stream);
                }
                Ok(_) => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if !waiting() {
                return Err(Error::new(ErrorKind::ConnectionAborted, "the debugger stopped before it connected"));
            }
            if Instant::now() >= deadline {
                return Err(Error::new(ErrorKind::TimedOut, "the debugger did not connect in time"));
            }
            std::thread::sleep(POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;

    #[test]
    fn the_first_loopback_connection_is_accepted() {
        let listener = ReverseListener::bind().unwrap();
        let addr = listener.addr();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            stream.write_all(b"hi").unwrap();
        });
        let mut stream = listener.accept(Duration::from_secs(5)).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        assert_eq!(text, "hi");
        client.join().unwrap();
        assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err());
    }

    #[test]
    fn accepting_times_out_or_stops_early() {
        let listener = ReverseListener::bind().unwrap();
        let started = Instant::now();
        let error = listener.accept(Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(50));
        let listener = ReverseListener::bind().unwrap();
        let error = listener.accept_while(Duration::from_secs(5), || false).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ConnectionAborted);
    }
}
