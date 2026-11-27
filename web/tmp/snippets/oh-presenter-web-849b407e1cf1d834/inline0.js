
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
