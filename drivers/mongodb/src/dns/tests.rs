use super::*;

fn name(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for label in text.split('.') {
        out.push(u8::try_from(label.len()).unwrap());
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out
}

fn answer(query: &[u8], records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut message = query.to_vec();
    message[2] = 0x81;
    message[3] = 0x80;
    message[6..8].copy_from_slice(&u16::try_from(records.len()).unwrap().to_be_bytes());
    for (kind, data) in records {
        message.extend_from_slice(&[0xc0, 12]);
        message.extend_from_slice(&kind.to_be_bytes());
        message.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
        message.extend_from_slice(&u16::try_from(data.len()).unwrap().to_be_bytes());
        message.extend_from_slice(data);
    }
    message
}

fn srv_data(port: u16, target: &str) -> Vec<u8> {
    let mut data = vec![0, 0, 0, 0];
    data.extend_from_slice(&port.to_be_bytes());
    data.extend(name(target));
    data
}

#[test]
fn name_servers_are_read_from_resolv_conf() {
    let servers = servers_of("# comment\nnameserver 10.0.0.1\nnameserver fe80::1%en0\nsearch local\n");
    assert_eq!(servers, ["10.0.0.1:53".parse().unwrap(), "[fe80::1]:53".parse().unwrap()]);
}

#[test]
fn answers_give_srv_targets_and_txt_strings() {
    let asked = query(7, "_mongodb._tcp.cluster0.example.net", SRV).unwrap();
    let message = answer(&asked, &[(SRV, srv_data(27017, "a.example.net")), (TXT, vec![3, b'a', b'=', b'b'])]);
    assert_eq!(parse(&message, 7, SRV).unwrap(), [Record::Srv("a.example.net".into(), 27017)]);
    assert_eq!(parse(&message, 7, TXT).unwrap(), [Record::Txt("a=b".into())]);
    assert!(parse(&message, 8, SRV).is_err(), "another query's answer");
    let mut missing = message.clone();
    missing[3] = 0x83;
    assert_eq!(parse(&missing, 7, SRV).unwrap_err(), "no such name");
    assert!(query(1, "bad..name", SRV).is_err());
}

#[tokio::test]
async fn an_srv_target_is_resolved_through_a_name_server() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buffer = [0u8; 512];
        for _ in 0..2 {
            let (read, from) = socket.recv_from(&mut buffer).await.unwrap();
            let query = &buffer[..read];
            let kind = u16::from_be_bytes([query[read - 4], query[read - 3]]);
            let records = if kind == SRV {
                vec![(SRV, srv_data(27017, "a.example.net")), (SRV, srv_data(27018, "evil.other.org"))]
            } else {
                let text = b"replicaSet=rs0&authSource=admin&tls=false";
                let mut data = vec![u8::try_from(text.len()).unwrap()];
                data.extend_from_slice(text);
                vec![(TXT, data)]
            };
            socket.send_to(&answer(query, &records), from).await.unwrap();
        }
    });
    let target = crate::target::parse_url("mongodb+srv://ann:pw@cluster0.example.net/shop").unwrap();
    let resolved = crate::connect::resolve_with(&[server], &target).await.unwrap();
    assert_eq!(resolved.hosts, [("a.example.net".to_owned(), 27017)], "hosts outside the parent domain are dropped");
    assert_eq!(resolved.replica_set.as_deref(), Some("rs0"));
    assert_eq!(resolved.tls["mode"], "verify-full", "TXT records cannot turn TLS off");
}
