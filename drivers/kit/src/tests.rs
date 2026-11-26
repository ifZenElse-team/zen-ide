use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value as Json, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

use super::*;

struct Echo;

struct EchoSession;

impl Driver for Echo {
    fn info(&self) -> DriverInfo {
        DriverInfo { id: "echo", name: "Echo", version: "1", capabilities: json!({ "models": ["relational"] }) }
    }

    fn connect(&self, params: Json) -> BoxFuture<'static, Result<Connected, Failure>> {
        Box::pin(async move {
            if params.get("password").and_then(Json::as_str) == Some("wrong") {
                return Err(Failure::database("authentication", "the password is wrong"));
            }
            Ok(Connected {
                server_version: Some("Echo 1".into()),
                in_transaction: false,
                session: Box::new(EchoSession),
            })
        })
    }
}

impl Session for EchoSession {
    fn run(self: Box<Self>, mut inbox: Inbox) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            while let Some(request) = inbox.next().await {
                let answer = match request.method.as_str() {
                    "ping" => Ok(json!({})),
                    "execute" => {
                        let statement = request.statement().map(str::to_owned);
                        let mut stop = inbox.stop(request.number("timeoutMs"));
                        match statement {
                            Ok(statement) if statement == "sleep" => {
                                stop.run(async {
                                    tokio::time::sleep(Duration::from_secs(30)).await;
                                    Ok(json!({}))
                                })
                                .await
                            }
                            Ok(statement) => Ok(json!({ "columns": [], "rows": [], "echo": statement })),
                            Err(failure) => Err(failure),
                        }
                    }
                    other => Err(Failure::method(other)),
                };
                inbox.answer(&request.id, answer);
            }
        })
    }
}

struct Client {
    input: DuplexStream,
    output: Lines<BufReader<DuplexStream>>,
    next: u64,
}

impl Client {
    fn start() -> Self {
        let (input, driver_input) = tokio::io::duplex(1 << 16);
        let (driver_output, output) = tokio::io::duplex(1 << 16);
        tokio::spawn(serve(Arc::new(Echo), driver_input, driver_output));
        Self { input, output: BufReader::new(output).lines(), next: 1 }
    }

    async fn write(&mut self, line: &str) {
        self.input.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    }

    async fn send(&mut self, method: &str, params: Json) -> u64 {
        let id = self.next;
        self.next += 1;
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        self.write(&line).await;
        id
    }

    async fn read(&mut self) -> Json {
        let line = tokio::time::timeout(Duration::from_secs(10), self.output.next_line())
            .await
            .expect("an answer in time")
            .unwrap()
            .expect("a line");
        serde_json::from_str(&line).unwrap()
    }

    async fn answer_to(&mut self, id: u64) -> Json {
        loop {
            let message = self.read().await;
            if message["id"] == id {
                return message;
            }
        }
    }

    async fn call(&mut self, method: &str, params: Json) -> Json {
        let id = self.send(method, params).await;
        let answer = self.answer_to(id).await;
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    }

    async fn initialized() -> Self {
        let mut client = Self::start();
        let result = client.call("initialize", json!({ "protocol": 1, "driver": "echo" })).await;
        assert_eq!(result["driver"]["id"], "echo");
        assert_eq!(result["capabilities"]["models"][0], "relational");
        client
    }
}

fn code(answer: &Json) -> i64 {
    answer["error"]["code"].as_i64().unwrap_or_else(|| panic!("an error: {answer}"))
}

#[tokio::test]
async fn requests_before_initialize_and_other_protocols_are_refused() {
    let mut client = Client::start();
    let id = client.send("ping", json!({ "session": "s1" })).await;
    assert_eq!(code(&client.answer_to(id).await), INVALID_REQUEST);
    let id = client.send("initialize", json!({ "protocol": 2 })).await;
    let answer = client.answer_to(id).await;
    assert_eq!(code(&answer), UNSUPPORTED_PROTOCOL);
    assert_eq!(answer["error"]["data"]["supported"], json!([1]));
}

#[tokio::test]
async fn a_broken_line_is_answered_and_the_program_goes_on() {
    let mut client = Client::initialized().await;
    client.write("{ not json").await;
    let answer = client.read().await;
    assert_eq!(code(&answer), PARSE_ERROR);
    assert_eq!(answer["id"], Json::Null);
    let id = client.send("zen/nothing", json!({})).await;
    assert_eq!(code(&client.answer_to(id).await), METHOD_NOT_FOUND);
}

#[tokio::test]
async fn sessions_connect_run_and_close() {
    let mut client = Client::initialized().await;
    let id = client.send("connect", json!({ "password": "wrong" })).await;
    assert_eq!(client.answer_to(id).await["error"]["data"]["kind"], "authentication");
    let connected = client.call("connect", json!({})).await;
    let session = connected["session"].as_str().unwrap().to_owned();
    assert_eq!(connected["serverVersion"], "Echo 1");
    let ran = client.call("execute", json!({ "session": session, "statement": "hello" })).await;
    assert_eq!(ran["echo"], "hello");
    let id = client.send("children", json!({ "session": session, "path": [] })).await;
    assert_eq!(code(&client.answer_to(id).await), METHOD_NOT_FOUND);
    client.call("close", json!({ "session": session })).await;
    let id = client.send("ping", json!({ "session": session })).await;
    assert_eq!(code(&client.answer_to(id).await), UNKNOWN_SESSION);
}

#[tokio::test]
async fn cancel_and_ping_are_answered_while_a_statement_runs() {
    let mut client = Client::initialized().await;
    let session = client.call("connect", json!({})).await["session"].as_str().unwrap().to_owned();
    let running = client.send("execute", json!({ "session": session, "statement": "sleep" })).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    client.call("ping", json!({ "session": session })).await;
    client.call("cancel", json!({ "session": session })).await;
    let answer = client.answer_to(running).await;
    assert_eq!(answer["error"]["data"]["kind"], "cancelled", "{answer}");
    let ran = client.call("execute", json!({ "session": session, "statement": "again" })).await;
    assert_eq!(ran["echo"], "again", "a cancel before a request does not stop it");
}

#[tokio::test]
async fn a_statement_past_its_timeout_fails_as_a_timeout() {
    let mut client = Client::initialized().await;
    let session = client.call("connect", json!({})).await["session"].as_str().unwrap().to_owned();
    let id = client.send("execute", json!({ "session": session, "statement": "sleep", "timeoutMs": 50 })).await;
    assert_eq!(client.answer_to(id).await["error"]["data"]["kind"], "timeout");
}

#[tokio::test]
async fn shutdown_is_answered_and_serving_ends() {
    let (mut input, driver_input) = tokio::io::duplex(1 << 16);
    let (driver_output, output) = tokio::io::duplex(1 << 16);
    let served = tokio::spawn(serve(Arc::new(Echo), driver_input, driver_output));
    let mut lines = BufReader::new(output).lines();
    for (id, method) in [(1, "initialize"), (2, "shutdown")] {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": { "protocol": 1 } });
        input.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        let answer: Json = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(answer["id"], id);
    }
    tokio::time::timeout(Duration::from_secs(5), served).await.expect("serving ends").unwrap();
}

#[test]
fn the_public_key_is_read_from_a_version_1_certificate_too() {
    let algorithm = [0x06, 0x03, 0x2b, 0x65, 0x70];
    let spki = [&[0x30, 0x0c, 0x30, 0x05][..], &algorithm, &[0x03, 0x03, 0x00, 0xaa, 0xbb]].concat();
    let fields = [&[0x02, 0x01, 0x01][..], &[0x30, 0x00], &[0x30, 0x00], &[0x30, 0x00], &[0x30, 0x00], &spki].concat();
    let tbs = [&[0x30, u8::try_from(fields.len()).unwrap()][..], &fields].concat();
    let certificate = [&[0x30, u8::try_from(tbs.len()).unwrap()][..], &tbs].concat();
    assert_eq!(tls::public_key(&certificate), Some((&algorithm[..], &[0xaa, 0xbb][..])));
    let versioned = [&[0xa0, 0x03, 0x02, 0x01, 0x02][..], &fields].concat();
    let tbs = [&[0x30, u8::try_from(versioned.len()).unwrap()][..], &versioned].concat();
    let certificate = [&[0x30, u8::try_from(tbs.len()).unwrap()][..], &tbs].concat();
    assert_eq!(tls::public_key(&certificate), Some((&algorithm[..], &[0xaa, 0xbb][..])));
    assert_eq!(tls::public_key(&[0x30, 0x05, 0x30]), None);
}
