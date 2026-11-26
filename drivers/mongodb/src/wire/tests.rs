use bson::doc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

use super::*;

fn message(id: i32, response_to: i32, body: &Document) -> Vec<u8> {
    let body = body.to_vec().unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&i32::try_from(HEADER + 5 + body.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&response_to.to_le_bytes());
    out.extend_from_slice(&OP_MSG.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&body);
    out
}

#[tokio::test]
async fn a_command_goes_out_as_op_msg_and_its_reply_comes_back() {
    let (client, mut server) = duplex(4096);
    let serve = tokio::spawn(async move {
        let mut header = [0u8; HEADER];
        server.read_exact(&mut header).await.unwrap();
        let length = usize::try_from(i32::from_le_bytes(header[..4].try_into().unwrap())).unwrap();
        let mut rest = vec![0u8; length - HEADER];
        server.read_exact(&mut rest).await.unwrap();
        let id = i32::from_le_bytes(header[4..8].try_into().unwrap());
        let command = body(&rest).unwrap();
        server.write_all(&message(99, id + 100, &doc! { "ok": 1, "stale": true })).await.unwrap();
        server.write_all(&message(100, id, &doc! { "ok": 1.0, "n": 3 })).await.unwrap();
        command
    });
    let mut wire = Wire::new(client);
    let reply = wire.command("shop", doc! { "count": "people" }).await.unwrap();
    assert_eq!(reply.get_i32("n").unwrap(), 3, "replies to other requests are skipped");
    assert_eq!(serve.await.unwrap(), doc! { "count": "people", "$db": "shop" });
}

#[test]
fn errors_in_replies_carry_the_servers_code() {
    let error = check(doc! { "ok": 0, "code": 13, "codeName": "Unauthorized", "errmsg": "not allowed" }).unwrap_err();
    let failure = error.failure();
    assert_eq!(failure.kind(), Some("refused"));
    assert!(failure.message.contains("not allowed") && failure.message.contains("Unauthorized"));
    let written = check(
        doc! { "ok": 1, "n": 0, "writeErrors": [{ "index": 0, "code": 11000, "errmsg": "E11000 duplicate key" }] },
    );
    let failure = written.unwrap_err().failure();
    assert_eq!(failure.kind(), Some("server"));
    assert_eq!(failure.data.as_ref().unwrap()["code"], "11000");
    assert!(WireError::Lost("x".into()).lost());
    assert!(body(&[0, 0, 0, 0]).is_err(), "a reply without a body");
}
