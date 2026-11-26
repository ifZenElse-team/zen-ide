#[cfg(not(target_arch = "wasm32"))]
pub mod bridge;
#[cfg(not(target_arch = "wasm32"))]
pub mod instances;
pub mod stats;

use std::sync::mpsc::Sender;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{self, Receiver};
#[cfg(not(target_arch = "wasm32"))]
use std::thread::JoinHandle;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

#[cfg(not(target_arch = "wasm32"))]
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(not(target_arch = "wasm32"))]
const LONGEST_WAIT: Duration = Duration::from_secs(600);

#[cfg(not(target_arch = "wasm32"))]
pub fn call_timeout(arguments: &Value) -> Duration {
    let asked = |value: &Value| value.get("waitMs").and_then(Value::as_u64);
    let wait = asked(arguments).or_else(|| arguments.get("params").and_then(asked)).unwrap_or(0);
    CALL_TIMEOUT + Duration::from_millis(wait).min(LONGEST_WAIT)
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
}

#[derive(Debug)]
pub struct Call {
    pub tool: String,
    pub arguments: Value,
    reply: Sender<Result<Value, String>>,
}

impl Call {
    pub fn reply(self, result: Result<Value, String>) {
        let _ = self.reply.send(result);
    }
}

#[derive(Clone, Debug)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub instructions: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Server {
    server: std::sync::Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
    address: std::net::SocketAddr,
}

#[cfg(not(target_arch = "wasm32"))]
impl Server {
    pub fn start(
        port: u16,
        info: ServerInfo,
        tools: Vec<Tool>,
        wake: impl Fn() + Send + 'static,
    ) -> Result<(Self, Receiver<Call>), String> {
        Self::start_counting(port, info, tools, std::sync::Arc::default(), wake)
    }

    pub fn start_counting(
        port: u16,
        info: ServerInfo,
        tools: Vec<Tool>,
        stats: std::sync::Arc<stats::Stats>,
        wake: impl Fn() + Send + 'static,
    ) -> Result<(Self, Receiver<Call>), String> {
        let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|error| error.to_string())?;
        let address = server.server_addr().to_ip().ok_or("the server has no IP address")?;
        let server = std::sync::Arc::new(server);
        let (calls, receiver) = mpsc::channel();
        let listener = std::sync::Arc::clone(&server);
        let thread = std::thread::Builder::new()
            .name("zen-automation".to_owned())
            .spawn(move || {
                for request in listener.incoming_requests() {
                    serve(request, &info, &tools, &calls, &stats, &wake);
                }
            })
            .map_err(|error| error.to_string())?;
        Ok((Self { server, thread: Some(thread), address }, receiver))
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.address)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Server {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn serve(
    mut request: tiny_http::Request,
    info: &ServerInfo,
    tools: &[Tool],
    calls: &Sender<Call>,
    stats: &stats::Stats,
    wake: &dyn Fn(),
) {
    let local_origin = request.headers().iter().find(|header| header.field.equiv("Origin")).is_none_or(|origin| {
        let origin = origin.value.as_str();
        ["http://127.0.0.1", "http://localhost", "https://127.0.0.1", "https://localhost"]
            .iter()
            .any(|local| origin == *local || origin.starts_with(&format!("{local}:")))
    });
    let respond = |request: tiny_http::Request, status: u16, body: Option<Value>| {
        let mut response = match body {
            Some(body) => tiny_http::Response::from_string(body.to_string()),
            None => tiny_http::Response::from_string(String::new()),
        }
        .with_status_code(status);
        if let Ok(header) = tiny_http::Header::from_bytes("Content-Type", "application/json") {
            response.add_header(header);
        }
        let _ = request.respond(response);
    };
    if !local_origin {
        respond(request, 403, Some(json!({ "error": "only local clients may connect" })));
        return;
    }
    if request.url() != "/mcp" {
        respond(request, 404, None);
        return;
    }
    if *request.method() != tiny_http::Method::Post {
        respond(request, 405, None);
        return;
    }
    let mut body = String::new();
    if std::io::Read::read_to_string(&mut request.as_reader(), &mut body).is_err() {
        respond(request, 400, None);
        return;
    }
    let message: Value = match serde_json::from_str(&body) {
        Ok(message) => message,
        Err(error) => {
            respond(request, 400, Some(rpc_error(Value::Null, -32700, &format!("parse error: {error}"))));
            return;
        }
    };
    match handle(&message, info, tools, calls, stats, wake) {
        Some(answer) => respond(request, 200, Some(answer)),
        None => respond(request, 202, None),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn handle(
    message: &Value,
    info: &ServerInfo,
    tools: &[Tool],
    calls: &Sender<Call>,
    stats: &stats::Stats,
    wake: &dyn Fn(),
) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": info.name, "version": info.version },
            "instructions": info.instructions,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools.iter().map(|tool| {
                let mut listed = json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                });
                if let Some(schema) = &tool.output_schema {
                    listed["outputSchema"] = schema.clone();
                }
                listed
            }).collect::<Vec<_>>()
        })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
            let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
                return Some(rpc_error(id, -32602, &format!("unknown tool `{name}`")));
            };
            let structured = tool.output_schema.is_some();
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let timeout = call_timeout(&arguments);
            let started = std::time::Instant::now();
            let counted = stats::counted_name(&name, &arguments);
            let bytes_in = arguments.to_string().len();
            let (reply, answer) = mpsc::channel();
            if calls.send(Call { tool: name, arguments, reply }).is_err() {
                return Some(rpc_error(id, -32603, "the application has stopped"));
            }
            wake();
            let outcome = answer
                .recv_timeout(timeout)
                .unwrap_or_else(|_| Err("the application did not answer in time".to_owned()));
            let mut result = match outcome {
                Ok(value) => tool_result(value, structured),
                Err(message) => json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
            };
            let elapsed = started.elapsed();
            let bytes_out = result["content"][0]["text"].as_str().map_or(0, str::len);
            stats.count(&counted, bytes_in, bytes_out, elapsed, result["isError"] == true);
            result["_meta"] = json!({ "elapsedMs": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX) });
            Ok(result)
        }
        other => Err(rpc_error(id.clone(), -32601, &format!("method `{other}` is not supported"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => error,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn tool_result(value: Value, structured: bool) -> Value {
    let mut result = json!({ "content": [{ "type": "text", "text": value.to_string() }], "isError": false });
    if structured {
        result["structuredContent"] = value;
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn post(url: &str, body: Value, origin: Option<&str>) -> (u16, Value) {
        let mut request = ureq::post(url).header("Content-Type", "application/json");
        if let Some(origin) = origin {
            request = request.header("Origin", origin);
        }
        match request.config().http_status_as_error(false).build().send(body.to_string()) {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let text = response.body_mut().read_to_string().unwrap_or_default();
                (status, serde_json::from_str(&text).unwrap_or(Value::Null))
            }
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn a_call_that_asks_to_wait_may_take_that_much_longer() {
        assert_eq!(call_timeout(&json!({})), CALL_TIMEOUT);
        let start = json!({ "command": "run.process.start", "params": { "waitMs": 120_000 } });
        assert_eq!(call_timeout(&start), CALL_TIMEOUT + Duration::from_secs(120));
        assert_eq!(call_timeout(&json!({ "waitMs": 5 })), CALL_TIMEOUT + Duration::from_millis(5));
        let forever = json!({ "params": { "waitMs": u64::MAX } });
        assert_eq!(call_timeout(&forever), CALL_TIMEOUT + LONGEST_WAIT);
    }

    #[test]
    fn speaks_mcp_and_forwards_tool_calls() {
        let tools = vec![Tool {
            name: "echo".to_owned(),
            description: "Echoes its arguments.".to_owned(),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
        }];
        let info =
            ServerInfo { name: "Test".to_owned(), version: "1.0".to_owned(), instructions: "Test server.".to_owned() };
        let (server, calls) = Server::start(0, info, tools, || {}).unwrap();
        let url = server.url();
        std::thread::spawn(move || {
            for call in calls {
                let arguments = call.arguments.clone();
                if arguments.get("fail").is_some() {
                    call.reply(Err("it failed".to_owned()));
                } else {
                    call.reply(Ok(arguments));
                }
            }
        });

        let (status, answer) =
            post(&url, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }), None);
        assert_eq!(status, 200);
        assert_eq!(answer["result"]["serverInfo"]["name"], "Test");
        let (status, _) = post(&url, json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), None);
        assert_eq!(status, 202);
        let (_, answer) = post(&url, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), None);
        assert_eq!(answer["result"]["tools"][0]["name"], "echo");
        let (_, answer) = post(
            &url,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "echo", "arguments": { "a": 1 } } }),
            Some("http://localhost:3000"),
        );
        assert_eq!(answer["result"]["content"], json!([{ "type": "text", "text": "{\"a\":1}" }]));
        assert!(answer["result"]["_meta"]["elapsedMs"].is_u64(), "the time, outside the payload: {answer}");
        let (_, answer) = post(
            &url,
            json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "echo", "arguments": { "fail": true } } }),
            None,
        );
        assert_eq!(answer["result"]["isError"], true);
        let (_, answer) = post(
            &url,
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "nope" } }),
            None,
        );
        assert_eq!(answer["error"]["code"], -32602);
        let (status, _) =
            post(&url, json!({ "jsonrpc": "2.0", "id": 6, "method": "ping" }), Some("https://evil.example"));
        assert_eq!(status, 403);
    }

    fn call_body(url: &str, tool: &str) -> String {
        let message =
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": tool, "arguments": {} } });
        let mut response =
            ureq::post(url).header("Content-Type", "application/json").send(message.to_string()).unwrap();
        response.body_mut().read_to_string().unwrap()
    }

    #[test]
    fn an_answer_carries_its_payload_once() {
        let tool = |name: &str, output_schema: Option<Value>| Tool {
            name: name.to_owned(),
            description: "Answers a list of problems.".to_owned(),
            input_schema: json!({ "type": "object" }),
            output_schema,
        };
        let schema = json!({ "type": "object", "properties": { "problems": { "type": "array" } } });
        let tools = vec![tool("plain", None), tool("typed", Some(schema.clone()))];
        let info = ServerInfo { name: "Test".to_owned(), version: "1.0".to_owned(), instructions: String::new() };
        let (server, calls) = Server::start(0, info, tools, || {}).unwrap();
        let url = server.url();
        let problems: Vec<Value> =
            (0..50).map(|line| json!({ "line": line, "message": "unused variable `x`" })).collect();
        let payload = json!({ "problems": problems });
        let reply = payload.clone();
        std::thread::spawn(move || {
            for call in calls {
                call.reply(Ok(reply.clone()));
            }
        });

        let body = call_body(&url, "plain");
        assert_eq!(body.matches("unused variable").count(), 50, "the payload is in the answer once: {body}");
        let answer: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(answer["result"].get("structuredContent"), None, "{answer}");
        let text = answer["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), payload);
        assert_eq!(text, payload.to_string(), "compact JSON");
        let size = payload.to_string().len();
        assert!(body.len() < size * 5 / 4, "{} bytes for a {size}-byte payload", body.len());

        let (_, listed) = post(&url, json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), None);
        assert_eq!(listed["result"]["tools"][0].get("outputSchema"), None);
        assert_eq!(listed["result"]["tools"][1]["outputSchema"], schema);
        let answer: Value = serde_json::from_str(&call_body(&url, "typed")).unwrap();
        assert_eq!(answer["result"]["structuredContent"], payload);
        let text = answer["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), payload);
    }
}
