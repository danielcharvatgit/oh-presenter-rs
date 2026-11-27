//! The browser's side: IndexedDB (bibles, pictures), opening and saving files, the output
//! window (a second browser window, fed over a `BroadcastChannel`), full screen, and the
//! WebMCP tools (`navigator.modelContext`) through which code agents control the page.
//!
//! Messages from the browser (a command of an agent, the output window asking for the
//! slide) wait in queues that the app empties every frame ([`take_commands`],
//! [`take_output_messages`]), so no Rust closure has to live in JavaScript.
//!
//! Outside the browser (`cargo test`) the same functions work on memory ([`native`]), so the
//! app can be tested without one.

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(inline_js = r##"
const DB = "oh-presenter";
let dbPromise = null;
function db() {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open(DB, 1);
      req.onupgradeneeded = () => req.result.createObjectStore("kv");
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  }
  return dbPromise;
}
function tx(mode, f) {
  return db().then((d) => new Promise((resolve, reject) => {
    const t = d.transaction("kv", mode);
    const r = f(t.objectStore("kv"));
    t.oncomplete = () => resolve(r && "result" in r ? r.result : undefined);
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  }));
}
export function idb_get(key) { return tx("readonly", (s) => s.get(key)).then((v) => v === undefined ? null : v); }
export function idb_set(key, value) { return tx("readwrite", (s) => s.put(value, key)); }
export function idb_del(key) { return tx("readwrite", (s) => s.delete(key)); }

export function ask_persistent_storage() {
  if (navigator.storage && navigator.storage.persist) navigator.storage.persist().catch(() => {});
}

export function pick_file(accept) {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    document.body.appendChild(input);
    let done = false;
    const finish = (v) => { if (!done) { done = true; input.remove(); resolve(v); } };
    input.addEventListener("change", async () => {
      const f = input.files && input.files[0];
      if (!f) return finish(null);
      finish([f.name, new Uint8Array(await f.arrayBuffer())]);
    });
    input.addEventListener("cancel", () => finish(null));
    input.click();
  });
}

export function download(name, bytes, mime) {
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

export function is_output_window() { return location.hash === "#output"; }

let outputWin = null;
export function open_output() {
  const url = location.href.split("#")[0] + "#output";
  outputWin = window.open(url, "oh-presenter-output", "popup,width=960,height=540");
  return !!outputWin;
}
export function close_output() {
  if (outputWin && !outputWin.closed) outputWin.close();
  else channel().postMessage({ close: true });
  outputWin = null;
}
export function output_open() { return !!outputWin && !outputWin.closed; }

let chan = null;
const outputQueue = [];
function channel() {
  if (!chan) {
    chan = new BroadcastChannel("oh-presenter-output");
    chan.onmessage = (e) => {
      if (e.data && e.data.close && is_output_window()) { window.close(); return; }
      outputQueue.push(typeof e.data === "string" ? e.data : JSON.stringify(e.data));
    };
  }
  return chan;
}
export function output_post(text) { channel().postMessage(text); }
export function take_output_messages() { channel(); return outputQueue.splice(0); }

export function toggle_fullscreen() {
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  else document.documentElement.requestFullscreen().catch(() => {});
}
export function is_fullscreen() { return !!document.fullscreenElement; }

// --- WebMCP: the page's tools for code agents -------------------------------------------
const pending = new Map();
const commandQueue = [];
let nextId = 1;
function call(name, args) {
  return new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    commandQueue.push(JSON.stringify({ id, name, args: args || {} }));
  });
}
export function take_commands() { return commandQueue.splice(0); }
export function answer_command(id, ok, text) {
  const r = pending.get(id);
  if (r) { pending.delete(id); r({ ok, text }); }
}
// tools: JSON [{name, description, inputSchema}]. Registered with navigator.modelContext
// (WebMCP) when the browser has it; always offered as window.ohPresenter for scripts and
// MCP bridges that drive the page.
export function register_tools(json) {
  const tools = JSON.parse(json);
  const execute = (name) => async (input) => {
    const r = await call(name, input);
    if (!r.ok) throw new Error(r.text);
    return { content: [{ type: "text", text: r.text }] };
  };
  window.ohPresenter = {
    tools,
    call: async (name, args) => { const r = await call(name, args); if (!r.ok) throw new Error(r.text); return r.text; },
  };
  const mc = navigator.modelContext;
  if (!mc) return 0;
  const defs = tools.map((t) => ({ ...t, execute: execute(t.name) }));
  try {
    if (typeof mc.provideContext === "function") mc.provideContext({ tools: defs });
    else if (typeof mc.registerTool === "function") defs.forEach((d) => mc.registerTool(d));
    else return 0;
  } catch (e) { console.warn("WebMCP:", e); return 0; }
  return defs.length;
}
"##)]
    extern "C" {
        #[wasm_bindgen(catch)]
        async fn idb_get(key: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch)]
        async fn idb_set(key: &str, value: JsValue) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch)]
        async fn idb_del(key: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(js_name = ask_persistent_storage)]
        fn ask_persistent_storage_js();
        async fn pick_file(accept: &str) -> JsValue;
        fn download(name: &str, bytes: &[u8], mime: &str);
        #[wasm_bindgen(js_name = is_output_window)]
        fn is_output_window_js() -> bool;
        #[wasm_bindgen(js_name = open_output)]
        fn open_output_js() -> bool;
        #[wasm_bindgen(js_name = close_output)]
        fn close_output_js();
        #[wasm_bindgen(js_name = output_open)]
        fn output_open_js() -> bool;
        #[wasm_bindgen(js_name = output_post)]
        fn output_post_js(text: &str);
        #[wasm_bindgen(js_name = take_output_messages)]
        fn take_output_messages_js() -> Vec<JsValue>;
        #[wasm_bindgen(js_name = toggle_fullscreen)]
        fn toggle_fullscreen_js();
        #[wasm_bindgen(js_name = is_fullscreen)]
        fn is_fullscreen_js() -> bool;
        #[wasm_bindgen(js_name = take_commands)]
        fn take_commands_js() -> Vec<JsValue>;
        #[wasm_bindgen(js_name = answer_command)]
        fn answer_command_js(id: f64, ok: bool, text: &str);
        #[wasm_bindgen(js_name = register_tools)]
        fn register_tools_js(json: &str) -> u32;
    }

    pub fn ask_persistent_storage() {
        ask_persistent_storage_js()
    }
    pub fn is_output_window() -> bool {
        is_output_window_js()
    }
    pub fn open_output() -> bool {
        open_output_js()
    }
    pub fn close_output() {
        close_output_js()
    }
    pub fn output_open() -> bool {
        output_open_js()
    }
    pub fn output_post(text: &str) {
        output_post_js(text)
    }
    pub fn toggle_fullscreen() {
        toggle_fullscreen_js()
    }
    pub fn is_fullscreen() -> bool {
        is_fullscreen_js()
    }
    pub fn answer_command(id: f64, ok: bool, text: &str) {
        answer_command_js(id, ok, text)
    }
    pub fn register_tools(json: &str) -> u32 {
        register_tools_js(json)
    }
    pub fn now_ms() -> f64 {
        js_sys::Date::now()
    }
    /// The browser's language code (`cs-CZ` → `cs`).
    pub fn browser_language() -> String {
        let lang = web_sys::window().and_then(|w| w.navigator().language()).unwrap_or_default();
        lang.split('-').next().unwrap_or("").to_lowercase()
    }
    pub fn spawn(f: impl std::future::Future<Output = ()> + 'static) {
        wasm_bindgen_futures::spawn_local(f)
    }

    /// Messages the output window (or the main window) sent since the last call.
    pub fn take_output_messages() -> Vec<String> {
        take_output_messages_js().into_iter().filter_map(|v| v.as_string()).collect()
    }

    /// Calls of the WebMCP tools since the last call: JSON `{id, name, args}`.
    pub fn take_commands() -> Vec<String> {
        take_commands_js().into_iter().filter_map(|v| v.as_string()).collect()
    }

    fn js_error(e: JsValue) -> String {
        e.as_string().or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string())).unwrap_or_else(|| "IndexedDB error".into())
    }

    /// A text stored in IndexedDB (bibles, which are too big for local storage).
    pub async fn db_get(key: &str) -> Result<Option<String>, String> {
        idb_get(key).await.map(|v| v.as_string()).map_err(js_error)
    }

    pub async fn db_set(key: &str, value: &str) -> Result<(), String> {
        idb_set(key, JsValue::from_str(value)).await.map(|_| ()).map_err(js_error)
    }

    /// Bytes stored in IndexedDB (pictures).
    pub async fn db_get_bytes(key: &str) -> Result<Option<Vec<u8>>, String> {
        let v = idb_get(key).await.map_err(js_error)?;
        Ok(v.dyn_ref::<js_sys::Uint8Array>().map(|a| a.to_vec()))
    }

    pub async fn db_set_bytes(key: &str, bytes: &[u8]) -> Result<(), String> {
        idb_set(key, js_sys::Uint8Array::from(bytes).into()).await.map(|_| ()).map_err(js_error)
    }

    pub async fn db_del(key: &str) -> Result<(), String> {
        idb_del(key).await.map(|_| ()).map_err(js_error)
    }

    /// Lets the user choose a file (`accept`: e.g. ".xml,.txt"): its name and bytes.
    pub async fn choose_file(accept: &str) -> Option<(String, Vec<u8>)> {
        let v = pick_file(accept).await;
        let a = v.dyn_ref::<js_sys::Array>()?;
        let name = a.get(0).as_string()?;
        let bytes = a.get(1).dyn_ref::<js_sys::Uint8Array>()?.to_vec();
        Some((name, bytes))
    }

    /// Offers `bytes` as a file to save (the browser's downloads).
    pub fn save_file(name: &str, bytes: &[u8], mime: &str) {
        download(name, bytes, mime);
    }

    /// Local storage (small data: settings, songs, the service). `Err` when it is full or off.
    pub fn local_get(key: &str) -> Option<String> {
        web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
    }

    pub fn local_set(key: &str, value: &str) -> Result<(), String> {
        let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or("no local storage")?;
        storage.set_item(key, value).map_err(js_error)
    }
}

/// Memory instead of the browser (tests): local storage, IndexedDB, the output channel and
/// the tool calls are lists that tests can fill and read.
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub mod native {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static LOCAL: RefCell<HashMap<String, String>> = RefCell::default();
        static DB: RefCell<HashMap<String, Vec<u8>>> = RefCell::default();
        /// Sent to the output window.
        pub static SENT: RefCell<Vec<String>> = RefCell::default();
        /// From the output window.
        pub static INBOX: RefCell<Vec<String>> = RefCell::default();
        pub static COMMANDS: RefCell<Vec<String>> = RefCell::default();
        pub static ANSWERS: RefCell<Vec<(f64, bool, String)>> = RefCell::default();
        static OUTPUT: RefCell<bool> = const { RefCell::new(false) };
        /// The file the next `choose_file` returns.
        pub static NEXT_FILE: RefCell<Option<(String, Vec<u8>)>> = RefCell::default();
        pub static SAVED_FILES: RefCell<Vec<(String, Vec<u8>)>> = RefCell::default();
    }

    pub async fn db_get(key: &str) -> Result<Option<String>, String> {
        Ok(DB.with(|d| d.borrow().get(key).map(|b| String::from_utf8_lossy(b).to_string())))
    }
    pub async fn db_set(key: &str, value: &str) -> Result<(), String> {
        DB.with(|d| d.borrow_mut().insert(key.into(), value.as_bytes().to_vec()));
        Ok(())
    }
    pub async fn db_get_bytes(key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(DB.with(|d| d.borrow().get(key).cloned()))
    }
    pub async fn db_set_bytes(key: &str, bytes: &[u8]) -> Result<(), String> {
        DB.with(|d| d.borrow_mut().insert(key.into(), bytes.to_vec()));
        Ok(())
    }
    pub async fn db_del(key: &str) -> Result<(), String> {
        DB.with(|d| d.borrow_mut().remove(key));
        Ok(())
    }
    pub fn ask_persistent_storage() {}
    pub async fn choose_file(_accept: &str) -> Option<(String, Vec<u8>)> {
        NEXT_FILE.with(|f| f.borrow_mut().take())
    }
    pub fn save_file(name: &str, bytes: &[u8], _mime: &str) {
        SAVED_FILES.with(|f| f.borrow_mut().push((name.into(), bytes.to_vec())));
    }
    pub fn local_get(key: &str) -> Option<String> {
        LOCAL.with(|l| l.borrow().get(key).cloned())
    }
    pub fn local_set(key: &str, value: &str) -> Result<(), String> {
        LOCAL.with(|l| l.borrow_mut().insert(key.into(), value.into()));
        Ok(())
    }
    pub fn is_output_window() -> bool {
        false
    }
    pub fn open_output() -> bool {
        OUTPUT.with(|o| *o.borrow_mut() = true);
        true
    }
    pub fn close_output() {
        OUTPUT.with(|o| *o.borrow_mut() = false);
    }
    pub fn output_open() -> bool {
        OUTPUT.with(|o| *o.borrow())
    }
    pub fn output_post(text: &str) {
        SENT.with(|s| s.borrow_mut().push(text.into()));
    }
    pub fn take_output_messages() -> Vec<String> {
        INBOX.with(|s| s.borrow_mut().drain(..).collect())
    }
    pub fn toggle_fullscreen() {}
    pub fn is_fullscreen() -> bool {
        false
    }
    pub fn take_commands() -> Vec<String> {
        COMMANDS.with(|s| s.borrow_mut().drain(..).collect())
    }
    pub fn answer_command(id: f64, ok: bool, text: &str) {
        ANSWERS.with(|s| s.borrow_mut().push((id, ok, text.into())));
    }
    pub fn register_tools(json: &str) -> u32 {
        serde_json::from_str::<Vec<serde_json::Value>>(json).map(|v| v.len() as u32).unwrap_or(0)
    }
    pub fn now_ms() -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    pub fn browser_language() -> String {
        "en".into()
    }
    /// Runs the future right away (the memory functions never wait).
    pub fn spawn(f: impl std::future::Future<Output = ()> + 'static) {
        let mut f = Box::pin(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        while f.as_mut().poll(&mut cx).is_pending() {}
    }
}
