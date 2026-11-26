use std::io::{self, Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const PROTOCOL: u32 = 1;

pub const OLDEST: u32 = 1;

const CONTROL: u8 = 0;
const DATA: u8 = 1;
const MAX_FRAME: usize = 4 << 20;

pub const MAX_DATA: usize = 1 << 20;

pub fn write_data(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    for chunk in bytes.chunks(MAX_DATA) {
        write_frame(out, &Frame::Data(chunk.to_vec()))?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Message {
    Hello {
        protocol: u32,
        oldest: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<u64>,
    },
    Open(OpenRequest),
    Attach {
        id: u64,
    },
    List,
    Resize {
        rows: u16,
        cols: u16,
    },
    Close,
    Keep {
        keep: bool,
    },
    Opened {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cols: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<u64>,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    Exited {
        code: Option<i32>,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub rows: u16,
    pub cols: u16,
    pub env: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preface: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: u64,
    pub pid: u32,
    pub cwd: PathBuf,
    pub attached: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Control(Message),
    Data(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Speaks {
    pub protocol: u32,
    pub oldest: u32,
}

impl Speaks {
    pub const CURRENT: Speaks = Speaks { protocol: PROTOCOL, oldest: OLDEST };

    pub fn meets(self, other: Speaks) -> Option<u32> {
        let common = self.protocol.min(other.protocol);
        (common >= self.oldest.max(other.oldest)).then_some(common)
    }
}

pub fn negotiate(protocol: u32, oldest: u32) -> Option<u32> {
    Speaks::CURRENT.meets(Speaks { protocol, oldest })
}

pub fn write_frame(out: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let (kind, payload) = match frame {
        Frame::Control(message) => (CONTROL, serde_json::to_vec(message).map_err(io::Error::other)?),
        Frame::Data(bytes) => (DATA, bytes.clone()),
    };
    let length = u32::try_from(payload.len() + 1).map_err(|_| io::Error::other("frame too large"))?;
    let mut buffer = Vec::with_capacity(payload.len() + 5);
    buffer.extend_from_slice(&length.to_be_bytes());
    buffer.push(kind);
    buffer.extend_from_slice(&payload);
    out.write_all(&buffer)?;
    out.flush()
}

pub fn read_frame(input: &mut impl Read) -> io::Result<Frame> {
    let mut header = [0; 4];
    input.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad frame length"));
    }
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    let payload = body.split_off(1);
    match body[0] {
        CONTROL => serde_json::from_slice(&payload)
            .map(Frame::Control)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        DATA => Ok(Frame::Data(payload)),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "unknown frame kind")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let frames = [
            Frame::Control(Message::Hello { protocol: PROTOCOL, oldest: OLDEST, instance: Some(7) }),
            Frame::Data(b"\x1b[31mred\r\n".to_vec()),
            Frame::Control(Message::Exited { code: Some(3) }),
        ];
        let mut wire = Vec::new();
        for frame in &frames {
            write_frame(&mut wire, frame).unwrap();
        }
        let mut input = wire.as_slice();
        for frame in &frames {
            assert_eq!(&read_frame(&mut input).unwrap(), frame);
        }
        assert!(read_frame(&mut input).is_err(), "the end of the stream");
    }

    #[test]
    fn versions_meet_at_the_newest_both_understand() {
        assert_eq!(negotiate(PROTOCOL, OLDEST), Some(PROTOCOL));
        assert_eq!(negotiate(PROTOCOL + 5, OLDEST), Some(PROTOCOL));
        assert_eq!(negotiate(PROTOCOL + 5, PROTOCOL + 1), None, "a newer side that dropped ours");
        assert_eq!(negotiate(0, 0), None, "an older side than our oldest");
        let newer = Speaks { protocol: 3, oldest: 2 };
        assert_eq!(newer.meets(Speaks { protocol: 2, oldest: 1 }), Some(2), "a newer IDE reads an older host");
        assert_eq!(newer.meets(Speaks { protocol: 1, oldest: 1 }), None, "one it no longer reads");
    }

    #[test]
    fn a_hello_without_an_instance_still_reads() {
        let old: Message = serde_json::from_str(r#"{"type":"hello","protocol":1,"oldest":1}"#).unwrap();
        assert_eq!(old, Message::Hello { protocol: 1, oldest: 1, instance: None });
    }

    #[test]
    fn data_larger_than_a_frame_goes_in_pieces_that_join_up() {
        let bytes: Vec<u8> = (0..(3 * MAX_DATA + 17)).map(|index| (index % 251) as u8).collect();
        let mut wire = Vec::new();
        write_data(&mut wire, &bytes).unwrap();
        let mut input = wire.as_slice();
        let mut joined = Vec::new();
        let mut pieces = 0;
        while !input.is_empty() {
            match read_frame(&mut input).unwrap() {
                Frame::Data(piece) => {
                    assert!(piece.len() <= MAX_DATA);
                    joined.extend(piece);
                    pieces += 1;
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!((pieces, joined), (4, bytes));
    }

    #[test]
    fn a_bad_length_is_refused() {
        let wire = [0u8, 0, 0, 0];
        assert!(read_frame(&mut wire.as_slice()).is_err());
        let huge = (u32::MAX).to_be_bytes();
        assert!(read_frame(&mut huge.as_slice()).is_err());
    }
}
