use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use zen_db_driver_kit::Failure;

const SRV: u16 = 33;
const TXT: u16 = 16;
const WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Record {
    Srv(String, u16),
    Txt(String),
}

pub(crate) fn name_servers() -> Vec<SocketAddr> {
    let text = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    servers_of(&text)
}

pub(crate) fn servers_of(text: &str) -> Vec<SocketAddr> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter_map(|address| address.split('%').next()?.parse::<std::net::IpAddr>().ok())
        .map(|address| SocketAddr::new(address, 53))
        .collect()
}

pub(crate) async fn srv(servers: &[SocketAddr], name: &str) -> Result<Vec<(String, u16)>, Failure> {
    let records = lookup(servers, name, SRV).await?;
    Ok(records
        .into_iter()
        .filter_map(|record| match record {
            Record::Srv(host, port) => Some((host, port)),
            Record::Txt(_) => None,
        })
        .collect())
}

pub(crate) async fn txt(servers: &[SocketAddr], name: &str) -> Result<Vec<String>, Failure> {
    let records = lookup(servers, name, TXT).await?;
    Ok(records
        .into_iter()
        .filter_map(|record| match record {
            Record::Txt(text) => Some(text),
            Record::Srv(..) => None,
        })
        .collect())
}

async fn lookup(servers: &[SocketAddr], name: &str, kind: u16) -> Result<Vec<Record>, Failure> {
    if servers.is_empty() {
        return Err(Failure::database("connection", "no DNS servers are known to look up a mongodb+srv:// address"));
    }
    let mut last = String::new();
    for server in servers {
        match ask(*server, name, kind).await {
            Ok(records) => return Ok(records),
            Err(error) => last = error,
        }
    }
    Err(Failure::database("connection", format!("DNS lookup of {name}: {last}")))
}

async fn ask(server: SocketAddr, name: &str, kind: u16) -> Result<Vec<Record>, String> {
    let mut id = [0u8; 2];
    getrandom::fill(&mut id).map_err(|error| error.to_string())?;
    let id = u16::from_be_bytes(id);
    let query = query(id, name, kind)?;
    let bind: SocketAddr = if server.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { ([0u16; 8], 0).into() };
    let socket = UdpSocket::bind(bind).await.map_err(|error| error.to_string())?;
    socket.connect(server).await.map_err(|error| error.to_string())?;
    socket.send(&query).await.map_err(|error| error.to_string())?;
    let mut buffer = vec![0u8; 4096];
    let answer = loop {
        let read = tokio::time::timeout(WAIT, socket.recv(&mut buffer))
            .await
            .map_err(|_| "no answer".to_owned())?
            .map_err(|error| error.to_string())?;
        if buffer.get(..2) == Some(&id.to_be_bytes()[..]) {
            break buffer[..read].to_vec();
        }
    };
    let answer =
        if answer.get(2).is_some_and(|flags| flags & 0x02 != 0) { over_tcp(server, &query).await? } else { answer };
    parse(&answer, id, kind)
}

async fn over_tcp(server: SocketAddr, query: &[u8]) -> Result<Vec<u8>, String> {
    let work = async {
        let mut stream = TcpStream::connect(server).await?;
        let mut framed = u16::try_from(query.len()).unwrap_or(u16::MAX).to_be_bytes().to_vec();
        framed.extend_from_slice(query);
        stream.write_all(&framed).await?;
        let mut length = [0u8; 2];
        stream.read_exact(&mut length).await?;
        let mut answer = vec![0u8; usize::from(u16::from_be_bytes(length))];
        stream.read_exact(&mut answer).await?;
        Ok::<_, std::io::Error>(answer)
    };
    tokio::time::timeout(WAIT, work).await.map_err(|_| "no answer".to_owned())?.map_err(|error| error.to_string())
}

pub(crate) fn query(id: u16, name: &str, kind: u16) -> Result<Vec<u8>, String> {
    let mut message = Vec::with_capacity(32 + name.len());
    message.extend_from_slice(&id.to_be_bytes());
    message.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        let length = u8::try_from(label.len()).ok().filter(|length| (1..64).contains(length));
        let length = length.ok_or_else(|| format!("`{name}` is not a host name"))?;
        message.push(length);
        message.extend_from_slice(label.as_bytes());
    }
    message.push(0);
    message.extend_from_slice(&kind.to_be_bytes());
    message.extend_from_slice(&1u16.to_be_bytes());
    Ok(message)
}

pub(crate) fn parse(message: &[u8], id: u16, kind: u16) -> Result<Vec<Record>, String> {
    let broken = || "the DNS answer is broken".to_owned();
    let number = |at: usize| message.get(at..at + 2).map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
    if number(0) != Some(id) {
        return Err(broken());
    }
    match message.get(3).map(|flags| flags & 0x0f) {
        Some(0) => {}
        Some(3) => return Err("no such name".into()),
        _ => return Err(format!("the DNS server failed ({})", message.get(3).map_or(0, |flags| flags & 0x0f))),
    }
    let questions = number(4).ok_or_else(broken)?;
    let answers = number(6).ok_or_else(broken)?;
    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(message, at).ok_or_else(broken)? + 4;
    }
    let mut records = Vec::new();
    for _ in 0..answers {
        at = skip_name(message, at).ok_or_else(broken)?;
        let record_kind = number(at).ok_or_else(broken)?;
        let length = usize::from(number(at + 8).ok_or_else(broken)?);
        let data_at = at + 10;
        let data = message.get(data_at..data_at + length).ok_or_else(broken)?;
        at = data_at + length;
        if record_kind != kind {
            continue;
        }
        if kind == SRV {
            let port = u16::from_be_bytes([*data.get(4).ok_or_else(broken)?, *data.get(5).ok_or_else(broken)?]);
            let target = read_name(message, data_at + 6).ok_or_else(broken)?;
            records.push(Record::Srv(target, port));
        } else {
            let mut text = Vec::new();
            let mut inner = 0;
            while let Some(&size) = data.get(inner) {
                let part = data.get(inner + 1..inner + 1 + usize::from(size)).ok_or_else(broken)?;
                text.extend_from_slice(part);
                inner += 1 + usize::from(size);
            }
            records.push(Record::Txt(String::from_utf8_lossy(&text).into_owned()));
        }
    }
    Ok(records)
}

fn skip_name(message: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let length = *message.get(at)?;
        if length & 0xc0 == 0xc0 {
            return Some(at + 2);
        }
        at += 1;
        if length == 0 {
            return Some(at);
        }
        at += usize::from(length);
    }
}

fn read_name(message: &[u8], mut at: usize) -> Option<String> {
    let mut labels = Vec::new();
    for _ in 0..128 {
        let length = *message.get(at)?;
        if length & 0xc0 == 0xc0 {
            at = usize::from(u16::from_be_bytes([length & 0x3f, *message.get(at + 1)?]));
            continue;
        }
        if length == 0 {
            return Some(labels.join("."));
        }
        let label = message.get(at + 1..at + 1 + usize::from(length))?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        at += 1 + usize::from(length);
    }
    None
}

#[cfg(test)]
mod tests;
