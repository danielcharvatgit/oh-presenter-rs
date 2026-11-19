//! Network outputs ("canvas" clients on other computers in the LAN).
//!
//! The presenter runs a TCP server; clients find it with a UDP broadcast (or
//! are given its address), connect, say hello and then receive JSON lines:
//! the current scene, plus the images it uses (sent once per client). Videos are sent
//! in pieces (`asset_part`), so a large file never has to be in memory at once.

use crate::model::{Alert, Scene};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 50200;
const DISCOVER: &[u8] = b"OH_PRESENTER_DISCOVER";
const ANNOUNCE: &str = "OH_PRESENTER_SERVER";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Scene {
        scene: Scene,
    },
    Asset {
        name: String,
        data: String,
    },
    /// A piece of a large file (a video): `offset` 0 starts the file, `last` completes it.
    AssetPart {
        name: String,
        offset: u64,
        data: String,
        last: bool,
    },
    /// Start (`Some`) or stop (`None`) an alert over the scene. (Older clients ignore it.)
    Alert {
        alert: Option<Alert>,
    },
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello { client_id: String, display_name: String },
    Pong,
}

#[derive(Clone, Debug)]
pub struct ClientInfo {
    pub id: String,
    pub display_name: String,
    pub addr: SocketAddr,
}

/// What the writer thread of a client sends.
enum Out {
    Line(String),
    /// A media file sent in pieces.
    File(String),
}

/// Size of one piece of a video sent to a client.
const PART_SIZE: usize = 1 << 20;

struct Client {
    info: ClientInfo,
    tx: Sender<Out>,
    sent_assets: HashSet<String>,
}

struct Shared {
    clients: HashMap<u64, Client>,
    scene: Scene,
    /// The running alert and when it started (for clients connecting meanwhile).
    alert: Option<(Alert, Instant)>,
    next_id: u64,
}

pub struct Server {
    shared: Arc<Mutex<Shared>>,
    pub error: Option<String>,
}

fn asset_msg(name: &str) -> Option<String> {
    let bytes = std::fs::read(crate::storage::media_path(name)).ok()?;
    let msg = ServerMsg::Asset { name: name.into(), data: base64::engine::general_purpose::STANDARD.encode(bytes) };
    serde_json::to_string(&msg).ok()
}

fn send_scene(client: &mut Client, scene: &Scene) {
    for m in scene.media() {
        if !client.sent_assets.insert(m.clone()) {
            continue;
        }
        if crate::video::is_video_name(&m) {
            let _ = client.tx.send(Out::File(m));
        } else if let Some(msg) = asset_msg(&m) {
            let _ = client.tx.send(Out::Line(msg));
        }
    }
    if let Ok(msg) = serde_json::to_string(&ServerMsg::Scene { scene: scene.clone() }) {
        let _ = client.tx.send(Out::Line(msg));
    }
}

/// Writes a media file to a client in pieces.
fn write_file(w: &mut impl Write, name: &str) -> std::io::Result<()> {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(crate::storage::media_path(name)) else { return Ok(()) };
    let len = f.metadata()?.len();
    let mut buf = vec![0u8; PART_SIZE];
    let mut offset = 0u64;
    loop {
        let mut n = 0;
        while n < buf.len() {
            match f.read(&mut buf[n..])? {
                0 => break,
                k => n += k,
            }
        }
        let last = offset + n as u64 >= len || n < buf.len();
        let msg = ServerMsg::AssetPart {
            name: name.into(),
            offset,
            data: base64::engine::general_purpose::STANDARD.encode(&buf[..n]),
            last,
        };
        w.write_all(serde_json::to_string(&msg).map_err(std::io::Error::other)?.as_bytes())?;
        w.write_all(b"\n")?;
        offset += n as u64;
        if last {
            return Ok(());
        }
    }
}

impl Server {
    /// Starts listening. Errors (port in use...) are kept in `error`, the app keeps working.
    pub fn start(port: u16) -> Self {
        let shared = Arc::new(Mutex::new(Shared { clients: HashMap::new(), scene: Scene::Blank, alert: None, next_id: 0 }));
        let mut server = Server { shared: shared.clone(), error: None };
        match TcpListener::bind(("0.0.0.0", port)) {
            Ok(listener) => {
                std::thread::spawn(move || {
                    for stream in listener.incoming().flatten() {
                        let shared = shared.clone();
                        std::thread::spawn(move || handle_client(stream, shared));
                    }
                });
            }
            Err(e) => server.error = Some(e.to_string()),
        }
        // discovery responder
        if let Ok(sock) = UdpSocket::bind(("0.0.0.0", port)) {
            std::thread::spawn(move || {
                let mut buf = [0u8; 64];
                while let Ok((n, from)) = sock.recv_from(&mut buf) {
                    if &buf[..n] == DISCOVER {
                        let _ = sock.send_to(format!("{ANNOUNCE} {port}").as_bytes(), from);
                    }
                }
            });
        }
        // keep-alive, drops dead clients
        let shared = server.shared.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let ping = serde_json::to_string(&ServerMsg::Ping).unwrap();
                shared.lock().unwrap().clients.retain(|_, c| c.tx.send(Out::Line(ping.clone())).is_ok());
            }
        });
        server
    }

    pub fn broadcast(&self, scene: &Scene) {
        let mut s = self.shared.lock().unwrap();
        s.scene = scene.clone();
        let scene = s.scene.clone();
        for c in s.clients.values_mut() {
            send_scene(c, &scene);
        }
    }

    /// Starts (or with `None` stops) an alert on every client.
    pub fn alert(&self, alert: Option<&Alert>) {
        let mut s = self.shared.lock().unwrap();
        s.alert = alert.map(|a| (a.clone(), Instant::now()));
        if let Ok(msg) = serde_json::to_string(&ServerMsg::Alert { alert: alert.cloned() }) {
            for c in s.clients.values_mut() {
                let _ = c.tx.send(Out::Line(msg.clone()));
            }
        }
    }

    pub fn clients(&self) -> Vec<ClientInfo> {
        let mut v: Vec<ClientInfo> = self.shared.lock().unwrap().clients.values().map(|c| c.info.clone()).collect();
        v.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        v
    }
}

fn handle_client(stream: TcpStream, shared: Arc<Mutex<Shared>>) {
    let Ok(addr) = stream.peer_addr() else { return };
    let _ = stream.set_nodelay(true);
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let Ok(ClientMsg::Hello { client_id, display_name }) = serde_json::from_str(&line) else { return };
    let (tx, rx) = channel::<Out>();
    let key = {
        let mut s = shared.lock().unwrap();
        let key = s.next_id;
        s.next_id += 1;
        let mut client = Client { info: ClientInfo { id: client_id, display_name, addr }, tx, sent_assets: HashSet::new() };
        let scene = s.scene.clone();
        send_scene(&mut client, &scene);
        // an alert that is running: the rest of it
        if let Some((alert, since)) = &s.alert {
            let mut a = alert.clone();
            let left = a.duration - since.elapsed().as_secs_f32();
            if a.duration == 0.0 || left > 0.5 {
                if a.duration > 0.0 {
                    a.duration = left;
                }
                if let Ok(msg) = serde_json::to_string(&ServerMsg::Alert { alert: Some(a) }) {
                    let _ = client.tx.send(Out::Line(msg));
                }
            }
        }
        s.clients.insert(key, client);
        key
    };
    // writer
    let mut write_half = stream;
    std::thread::spawn(move || {
        for out in rx {
            let res = match out {
                Out::Line(msg) => write_half.write_all(msg.as_bytes()).and_then(|_| write_half.write_all(b"\n")),
                Out::File(name) => write_file(&mut write_half, &name),
            };
            if res.is_err() {
                break;
            }
        }
        let _ = write_half.shutdown(std::net::Shutdown::Both);
    });
    // reader: lives until the client goes away
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    shared.lock().unwrap().clients.remove(&key);
}

// ---------------------------------------------------------------------------
// Client side

pub enum ClientEvent {
    Connected(SocketAddr),
    Disconnected,
    Scene(Scene),
    /// An image arrived and was stored in the client media directory.
    Asset(String),
    Alert(Option<Alert>),
}

/// A received media name made safe as a file name in the client media folder.
fn client_media_name(name: String) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') && !name.starts_with('.') {
        name
    } else {
        crate::storage::safe_file_name(&name).replace(' ', "_")
    }
}

/// Finds a server with a UDP broadcast. Returns its TCP address.
fn discover(port: u16) -> Option<SocketAddr> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.set_broadcast(true).ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(700))).ok()?;
    for target in [format!("255.255.255.255:{port}"), format!("127.0.0.1:{port}")] {
        let _ = sock.send_to(DISCOVER, target);
    }
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            let msg = String::from_utf8_lossy(&buf[..n]);
            if let Some(p) = msg.strip_prefix(ANNOUNCE).and_then(|p| p.trim().parse::<u16>().ok()) {
                return Some(SocketAddr::new(from.ip(), p));
            }
        }
    }
    None
}

/// Runs the client connection loop in a background thread, reconnecting forever.
/// `server` = explicit address, otherwise the server is discovered on the LAN.
pub fn run_client(server: Option<String>, display_name: String, ctx: eframe::egui::Context) -> Receiver<ClientEvent> {
    let (tx, rx) = channel();
    let client_id = crate::model::make_id();
    std::thread::spawn(move || {
        loop {
            let addr = match &server {
                Some(s) => {
                    let s = if s.contains(':') { s.clone() } else { format!("{s}:{DEFAULT_PORT}") };
                    std::net::ToSocketAddrs::to_socket_addrs(&s).ok().and_then(|mut a| a.next())
                }
                None => discover(DEFAULT_PORT),
            };
            if let Some(addr) = addr
                && let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
            {
                let _ = tx.send(ClientEvent::Connected(addr));
                ctx.request_repaint();
                client_session(stream, &client_id, &display_name, &tx, &ctx);
                let _ = tx.send(ClientEvent::Disconnected);
                ctx.request_repaint();
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    rx
}

fn client_session(mut stream: TcpStream, client_id: &str, display_name: &str, tx: &Sender<ClientEvent>, ctx: &eframe::egui::Context) {
    let hello = ClientMsg::Hello { client_id: client_id.into(), display_name: display_name.into() };
    if stream.write_all(format!("{}\n", serde_json::to_string(&hello).unwrap()).as_bytes()).is_err() {
        return;
    }
    // the server pings every 3 s; silence for longer means it is gone
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(read_half) = stream.try_clone() else { return };
    let reader = BufReader::new(read_half);
    let media_dir = crate::storage::paths().client_media();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        match serde_json::from_str::<ServerMsg>(&line) {
            Ok(ServerMsg::Scene { scene }) => {
                let _ = tx.send(ClientEvent::Scene(scene));
            }
            Ok(ServerMsg::Asset { name, data }) => {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                    let name = client_media_name(name);
                    let _ = crate::storage::write_atomic(&media_dir.join(&name), &bytes, false);
                    let _ = tx.send(ClientEvent::Asset(name));
                }
            }
            Ok(ServerMsg::AssetPart { name, offset, data, last }) => {
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else { continue };
                let name = client_media_name(name);
                let part = media_dir.join(format!("{name}.part"));
                let written = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(offset > 0)
                    .truncate(offset == 0)
                    .open(&part)
                    .and_then(|mut f| f.write_all(&bytes));
                if written.is_err() || !last {
                    continue;
                }
                // complete: only now does the video appear under its name
                if std::fs::rename(&part, media_dir.join(&name)).is_ok() {
                    let _ = tx.send(ClientEvent::Asset(name));
                }
            }
            Ok(ServerMsg::Alert { alert }) => {
                let _ = tx.send(ClientEvent::Alert(alert));
            }
            Ok(ServerMsg::Ping) => {
                let _ = stream.write_all(format!("{}\n", serde_json::to_string(&ClientMsg::Pong).unwrap()).as_bytes());
                continue;
            }
            Err(_) => continue,
        }
        ctx.request_repaint();
    }
}
