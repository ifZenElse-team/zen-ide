use std::sync::Arc;

use ed25519_dalek::{Signer, SigningKey};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use super::*;

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    match content.len() {
        length if length < 128 => out.push(u8::try_from(length).unwrap()),
        length if length < 256 => out.extend([0x81, u8::try_from(length).unwrap()]),
        length => out.extend([0x82, u8::try_from(length >> 8).unwrap(), u8::try_from(length & 0xff).unwrap()]),
    }
    out.extend_from_slice(content);
    out
}

fn sequence(parts: &[Vec<u8>]) -> Vec<u8> {
    der(0x30, &parts.concat())
}

fn certificate(key: &SigningKey) -> Vec<u8> {
    let ed25519 = sequence(&[der(0x06, &[0x2b, 0x65, 0x70])]);
    let name = sequence(&[der(0x31, &sequence(&[der(0x06, &[0x55, 0x04, 0x03]), der(0x0c, b"localhost")]))]);
    let validity = sequence(&[der(0x17, b"200101000000Z"), der(0x18, b"20991231235959Z")]);
    let mut public = vec![0];
    public.extend_from_slice(key.verifying_key().as_bytes());
    let key_info = sequence(&[ed25519.clone(), der(0x03, &public)]);
    let to_sign = sequence(&[
        der(0xa0, &der(0x02, &[2])),
        der(0x02, &[1]),
        ed25519.clone(),
        name.clone(),
        validity,
        name,
        key_info,
    ]);
    let mut signature = vec![0];
    signature.extend_from_slice(&key.sign(&to_sign).to_bytes());
    sequence(&[to_sign, ed25519, der(0x03, &signature)])
}

fn server_config(alpn: Option<&[u8]>) -> Arc<ServerConfig> {
    let key = SigningKey::from_bytes(&[7; 32]);
    let mut pkcs8 =
        vec![0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20];
    pkcs8.extend_from_slice(&key.to_bytes());
    let mut config = ServerConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
        .with_protocol_versions(&[&rustls::version::TLS12])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate(&key))],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8)),
        )
        .unwrap();
    if let Some(alpn) = alpn {
        config.alpn_protocols = vec![alpn.to_vec()];
    }
    Arc::new(config)
}

fn client_config(alpn: &[&[u8]]) -> Arc<ClientConfig> {
    zen_db_driver_kit::tls::client_config(&json!({ "mode": "require" }), alpn).unwrap()
}

fn name() -> ServerName<'static> {
    ServerName::try_from("localhost").unwrap()
}

fn prelogin_request() -> Vec<u8> {
    let payload = [0x01, 0x00, 0x06, 0x00, 0x01, 0xff, 0x01];
    let mut packet = vec![PRELOGIN, 0x01, 0x00, 15, 0, 0, 1, 0];
    packet.extend_from_slice(&payload);
    packet
}

fn prelogin_answer(level: u8) -> Vec<u8> {
    let payload = [0x00, 0x00, 0x0b, 0x00, 0x06, 0x01, 0x00, 0x11, 0x00, 0x01, 0xff, 16, 0, 0, 0, 0, 0, level];
    let mut packet = vec![0x04, 0x01, 0x00, 26, 0, 0, 1, 0];
    packet.extend_from_slice(&payload);
    packet
}

async fn read_packet(stream: &mut (impl AsyncRead + Unpin)) -> Vec<u8> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await.unwrap();
    let length = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let mut body = vec![0; length - 8];
    stream.read_exact(&mut body).await.unwrap();
    [header.to_vec(), body].concat()
}

async fn listener() -> (TcpListener, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

async fn client(address: std::net::SocketAddr, wanted: Wanted) -> Link<TcpStream> {
    let tcp = TcpStream::connect(address).await.unwrap();
    let mut link = Link::negotiated(tcp, client_config(&[]), name(), wanted);
    link.write_all(&prelogin_request()).await.unwrap();
    link.flush().await.unwrap();
    read_packet(&mut link).await;
    link
}

#[test]
fn the_encryption_level_is_read_from_the_options() {
    assert_eq!(encryption_of(&prelogin_answer(3)[8..]), Some(3));
    assert_eq!(encryption_of(&prelogin_answer(0x81)[8..]), Some(1), "the client certificate bit is not a level");
    assert_eq!(encryption_of(&[0x00, 0x00, 0x06, 0x00, 0x01, 0xff, 0x09]), None);
    let mut answer = Answer::default();
    let packet = prelogin_answer(2);
    answer.feed(&packet[..5]);
    assert_eq!(answer.encryption, None, "half a packet decides nothing");
    answer.feed(&packet[5..]);
    assert_eq!(answer.encryption, Some(2));
}

#[tokio::test]
async fn long_handshake_writes_are_cut_into_prelogin_packets() {
    let (listener, address) = listener().await;
    let reader = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let first = read_packet(&mut tcp).await;
        let second = read_packet(&mut tcp).await;
        (first, second)
    });
    let mut framed = Framed::new(TcpStream::connect(address).await.unwrap());
    framed.wrap(true);
    framed.write_all(&vec![9; 5000]).await.unwrap();
    framed.flush().await.unwrap();
    let (first, second) = reader.await.unwrap();
    assert_eq!((first[0], first[1], first.len()), (PRELOGIN, 0, PACKET));
    assert_eq!((second[0], second[1], second.len()), (PRELOGIN, 1, 5000 - (PACKET - HEADER) + HEADER));
    assert_eq!(second[6], first[6].wrapping_add(1), "packets are numbered");
}

#[tokio::test]
async fn tls_after_the_prelogin_travels_in_prelogin_packets_and_then_as_records() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut framed = Framed::new(tcp);
        let request = read_packet(&mut framed).await;
        assert_eq!(request[0], PRELOGIN);
        framed.write_all(&prelogin_answer(1)).await.unwrap();
        framed.flush().await.unwrap();
        framed.wrap(true);
        let mut tls = TlsAcceptor::from(server_config(None)).accept(framed).await.unwrap();
        tls.get_mut().0.wrap(false);
        let mut login = [0u8; 5];
        tls.read_exact(&mut login).await.unwrap();
        tls.write_all(b"OK").await.unwrap();
        tls.flush().await.unwrap();
        login
    });
    let mut link = client(address, Wanted::Always).await;
    link.write_all(b"LOGIN").await.unwrap();
    link.flush().await.unwrap();
    let mut answer = [0u8; 2];
    link.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"OK");
    assert_eq!(&server.await.unwrap(), b"LOGIN", "the login went through TLS");
}

#[tokio::test]
async fn a_server_without_tls_is_used_plain_when_tls_is_only_preferred() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        read_packet(&mut tcp).await;
        tcp.write_all(&prelogin_answer(NOT_SUPPORTED)).await.unwrap();
        let mut login = [0u8; 5];
        tcp.read_exact(&mut login).await.unwrap();
        login
    });
    let mut link = client(address, Wanted::IfOffered).await;
    link.write_all(b"LOGIN").await.unwrap();
    link.flush().await.unwrap();
    assert_eq!(&server.await.unwrap(), b"LOGIN");
}

#[tokio::test]
async fn a_server_that_only_offers_tls_is_used_plain_when_tls_is_off() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        read_packet(&mut tcp).await;
        tcp.write_all(&prelogin_answer(0)).await.unwrap();
        let mut login = [0u8; 5];
        tcp.read_exact(&mut login).await.unwrap();
        login
    });
    let mut link = client(address, Wanted::Never).await;
    link.write_all(b"LOGIN").await.unwrap();
    link.flush().await.unwrap();
    assert_eq!(&server.await.unwrap(), b"LOGIN");
}

#[tokio::test]
async fn a_mismatch_of_what_each_side_wants_fails_before_the_login_is_sent() {
    for (level, wanted) in [(NOT_SUPPORTED, Wanted::Always), (REQUIRED, Wanted::Never)] {
        let (listener, address) = listener().await;
        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            read_packet(&mut tcp).await;
            tcp.write_all(&prelogin_answer(level)).await.unwrap();
            let mut rest = Vec::new();
            let _ = tcp.read_to_end(&mut rest).await;
            rest
        });
        let mut link = client(address, wanted).await;
        assert!(link.write_all(b"LOGIN").await.is_err(), "{level} with {wanted:?}");
        drop(link);
        assert!(server.await.unwrap().is_empty(), "nothing was sent in the clear");
    }
}

#[tokio::test]
async fn strict_tls_comes_first_and_offers_tds_8() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = TlsAcceptor::from(server_config(Some(b"tds/8.0"))).accept(tcp).await.unwrap();
        let alpn = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
        let mut first = [0u8; 1];
        tls.read_exact(&mut first).await.unwrap();
        (alpn, first[0])
    });
    let tcp = TcpStream::connect(address).await.unwrap();
    let mut link = Link::strict(tcp, client_config(&[b"tds/8.0"]), name()).await.unwrap();
    link.write_all(&prelogin_request()).await.unwrap();
    link.flush().await.unwrap();
    let (alpn, first) = server.await.unwrap();
    assert_eq!(alpn.as_deref(), Some(&b"tds/8.0"[..]));
    assert_eq!(first, PRELOGIN, "the pre-login request is the first thing inside TLS");
}

#[tokio::test]
async fn the_pre_login_request_asks_for_the_encryption_the_link_does() {
    for (wanted, level) in [(Wanted::Never, NOT_SUPPORTED), (Wanted::IfOffered, ON), (Wanted::Always, ON)] {
        let (listener, address) = listener().await;
        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            read_packet(&mut tcp).await
        });
        let tcp = TcpStream::connect(address).await.unwrap();
        let mut link = Link::negotiated(tcp, client_config(&[]), name(), wanted);
        let request = prelogin_request();
        link.write_all(&request[..5]).await.unwrap();
        link.write_all(&request[5..]).await.unwrap();
        link.flush().await.unwrap();
        let sent = server.await.unwrap();
        assert_eq!(encryption_of(&sent[HEADER..]), Some(level), "{wanted:?}");
    }
}

fn done(status: u8) -> Vec<u8> {
    let mut packet = vec![0x04, 0x01, 0x00, 21, 0, 0, 1, 0, DONE, status, 0];
    packet.extend_from_slice(&[0; 10]);
    packet
}

#[tokio::test]
async fn after_an_attention_the_messages_before_its_acknowledgement_read_as_one() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        read_packet(&mut tcp).await;
        tcp.write_all(&prelogin_answer(NOT_SUPPORTED)).await.unwrap();
        let attention = read_packet(&mut tcp).await;
        tcp.write_all(&[done(0x02), done(DONE_ATTENTION), done(0x00)].concat()).await.unwrap();
        attention
    });
    let mut link = client(address, Wanted::IfOffered).await;
    link.write_all(&[ATTENTION, 0x01, 0, 8, 0, 0, 1, 0]).await.unwrap();
    link.flush().await.unwrap();
    let first = read_packet(&mut link).await;
    let second = read_packet(&mut link).await;
    let third = read_packet(&mut link).await;
    assert_eq!(server.await.unwrap()[0], ATTENTION);
    assert_eq!(first[1] & 1, 0, "the message cut short by the attention goes on");
    assert_eq!(second[1] & 1, 1, "the acknowledgement ends it");
    assert_eq!(third[1] & 1, 1, "later messages are left alone");
}
