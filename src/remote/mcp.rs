//! MCP server for code agents (Settings › Advanced › MCP server): the Model Context Protocol
//! over "Streamable HTTP" with plain JSON answers, at `http://127.0.0.1:<port>/mcp`.
//! Every command of [`super::COMMANDS`] is a tool; a tool call runs the command in the UI
//! thread as if the operator had pressed the button.
//!
//! It listens on 127.0.0.1 only and refuses requests a web page could send (an `Origin` or
//! `Host` that is not this computer), so only programs on this computer can use it.
//!
//! Connect an agent, e.g. Claude Code: `claude mcp add --transport http oh-presenter http://127.0.0.1:7779/mcp`.

use super::{COMMANDS, Sender, check_args, input_schema, spec};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 7779;
/// Protocol versions this server speaks (the newest first).
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY: usize = 1 << 20;
/// Longest request or header line.
const MAX_LINE: u64 = 16 * 1024;
const MAX_CONNECTIONS: usize = 16;
/// Connections being served now.
static OPEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Runs a command in the UI thread (see [`Sender::call`]).
pub type Call<'a> = dyn Fn(&str, Map<String, Value>) -> Result<String, String> + 'a;

/// The running server; dropping it stops it.
pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start(port: u16, sender: Sender) -> Result<Server, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| format!("127.0.0.1:{port}: {e}"))?;
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                // a few agents at a time is plenty; more connections are closed right away
                if OPEN.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    continue;
                }
                OPEN.fetch_add(1, Ordering::SeqCst);
                let (sender, stopped) = (sender.clone(), stopped.clone());
                std::thread::spawn(move || {
                    let _ = serve(conn, &stopped, &sender);
                    OPEN.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        crate::log_info(&format!("MCP server on http://127.0.0.1:{port}/mcp"));
        Ok(Server { port, stop })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // wakes the accept loop up, so it sees the stop
        let _ = TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, self.port).into(), Duration::from_millis(200));
    }
}

/// An HTTP request as far as this server needs it.
#[derive(Debug, Default)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// One line, at most [`MAX_LINE`] long.
fn read_line(r: &mut impl BufRead, line: &mut String) -> Result<(), String> {
    line.clear();
    let n = r.by_ref().take(MAX_LINE).read_line(line).map_err(|e| e.to_string())?;
    if n as u64 >= MAX_LINE && !line.ends_with('\n') {
        return Err("line too long".into());
    }
    Ok(())
}

fn read_request(r: &mut impl BufRead) -> Result<HttpRequest, String> {
    let mut line = String::new();
    read_line(r, &mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string());
    if method.is_empty() {
        return Err("empty request".into());
    }
    let mut req = HttpRequest { method, path, ..Default::default() };
    loop {
        read_line(r, &mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if req.headers.len() > 100 {
            return Err("too many headers".into());
        }
        if let Some((k, v)) = l.split_once(':') {
            req.headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    if req.header("transfer-encoding").is_some() {
        return Err("chunked bodies are not supported: send Content-Length".into());
    }
    let len: usize = req.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err("body too large".into());
    }
    req.body = vec![0; len];
    r.read_exact(&mut req.body).map_err(|e| e.to_string())?;
    Ok(req)
}

/// Whether a host name (from `Origin` or `Host`, maybe with a port) is this computer.
fn is_local_host(host: &str) -> bool {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').map(|(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host)
    };
    matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// Whether the request comes from a program on this computer, not from a web page (DNS
/// rebinding: a page of another site could otherwise reach 127.0.0.1).
pub fn allowed(req: &HttpRequest) -> bool {
    let origin_ok = match req.header("origin") {
        None => true,
        Some(o) => {
            let rest = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://"));
            rest.is_some_and(|r| is_local_host(r.split('/').next().unwrap_or_default()))
        }
    };
    let host_ok = req.header("host").is_none_or(is_local_host);
    origin_ok && host_ok
}

/// The HTTP answer: status, content type, body.
pub fn respond(req: &HttpRequest, sender: &Call<'_>) -> (u16, &'static str, String) {
    if !allowed(req) {
        return (403, "text/plain", "only programs on this computer may use this server".into());
    }
    if req.path.split('?').next() != Some("/mcp") {
        return (404, "text/plain", "the MCP endpoint is /mcp".into());
    }
    match req.method.as_str() {
        "POST" => {}
        // no server-sent events: everything is answered right away
        "GET" | "DELETE" => return (405, "text/plain", "only POST".into()),
        _ => return (405, "text/plain", "only POST".into()),
    }
    let msg: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(e) => return (400, "application/json", error(Value::Null, -32700, &format!("parse error: {e}")).to_string()),
    };
    let answers: Vec<Value> = match &msg {
        Value::Array(list) => list.iter().filter_map(|m| handle(m, sender)).collect(),
        m => handle(m, sender).into_iter().collect(),
    };
    match (&msg, answers.len()) {
        // only notifications / answers of the client: accepted, nothing to say
        (_, 0) => (202, "application/json", String::new()),
        (Value::Array(_), _) => (200, "application/json", Value::Array(answers).to_string()),
        _ => (200, "application/json", answers[0].to_string()),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// The tools: every command.
pub fn tools() -> Vec<Value> {
    COMMANDS.iter().map(|s| json!({"name": s.name, "description": s.help, "inputSchema": input_schema(s)})).collect()
}

/// One JSON-RPC message; `None` for a notification (no answer).
fn handle(msg: &Value, call: &Call<'_>) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    let (Some(id), Some(method)) = (id, method) else {
        // a notification, or a client's answer to something: nothing to answer
        return None;
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[1]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "oh-presenter-rs", "title": "oh! Presenter", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Controls the oh! Presenter program like its operator would: the service, slides, videos, the output window, alerts, bibles and settings. Start with status and list-items.",
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
            let Some(spec) = spec(name) else { return Some(error(id, -32602, &format!("unknown tool {name:?}"))) };
            let out = check_args(spec, &args).and_then(|()| call(name, args));
            let (text, is_error) = match out {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            result(id, json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

fn serve(conn: TcpStream, stopped: &AtomicBool, sender: &Sender) -> Result<(), String> {
    conn.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| e.to_string())?;
    let mut writer = conn.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(conn);
    // keep-alive: several requests on one connection
    loop {
        let req = match read_request(&mut reader) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };
        // switched off meanwhile: agents that are still connected lose control too
        if stopped.load(Ordering::SeqCst) {
            return Ok(());
        }
        let call = |name: &str, args: Map<String, Value>| sender.call(name, args, Duration::from_secs(20));
        let (status, ctype, body) = respond(&req, &call);
        let reason = match status {
            200 => "OK",
            202 => "Accepted",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            _ => "Method Not Allowed",
        };
        let allow = if status == 405 { "Allow: POST\r\n" } else { "" };
        let head = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n{allow}\r\n", body.len());
        writer.write_all(head.as_bytes()).and_then(|()| writer.write_all(body.as_bytes())).map_err(|e| e.to_string())?;
        if req.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close")) {
            return Ok(());
        }
    }
}
