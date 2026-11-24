//! Controlling the program without its window: the command line (`--cli`, commands typed on
//! stdin) and the MCP server for code agents ([`mcp`]) share one set of commands, named
//! after the buttons of the program (in English). A command runs in the UI thread as if the
//! operator had pressed that button (see `App::run_command`).

pub mod mcp;

use serde_json::{Map, Value, json};
use std::sync::mpsc;

/// The kind of a command's parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Int,
    Number,
    Bool,
    /// One of these words.
    OneOf(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub help: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub help: &'static str,
    pub params: &'static [Param],
}

const fn p(name: &'static str, kind: Kind, required: bool, help: &'static str) -> Param {
    Param { name, kind, required, help }
}

const ON_OFF: Kind = Kind::OneOf(&["on", "off"]);

/// Values of `playback-control` (CLI `--playback-control=…`).
pub const PLAYBACK: &[&str] =
    &["play", "pause", "play-pause", "play-loop", "play-once", "restart", "seek", "forward", "back", "mute", "unmute"];

/// Every command. The order is the order of `help`.
pub const COMMANDS: &[Spec] = &[
    // Service
    Spec {
        name: "new-service",
        help: "New service (refused when the service has unsaved changes, unless discard=true)",
        params: &[p("discard", Kind::Bool, false, "drop unsaved changes")],
    },
    Spec {
        name: "open-service",
        help: "Open service… (.ohsvc, OpenLP .osz / .oszl, old .ohpres)",
        params: &[p("file", Kind::Text, true, "path of the service file"), p("discard", Kind::Bool, false, "drop unsaved changes")],
    },
    Spec { name: "save-service", help: "Save service (to its file; a new service needs save-service-as)", params: &[] },
    Spec { name: "save-service-as", help: "Save service as…", params: &[p("file", Kind::Text, true, "path of the new .ohsvc file")] },
    Spec {
        name: "add-song",
        help: "Add a song of the library to the service (after the selected item)",
        params: &[p("title", Kind::Text, true, "song title (or the beginning of it)")],
    },
    Spec { name: "add-design", help: "Add a design of the library to the service", params: &[p("name", Kind::Text, true, "design name")] },
    Spec {
        name: "add-text",
        help: "Add a text item (a line with only --- starts a new slide)",
        params: &[p("title", Kind::Text, true, "title"), p("text", Kind::Text, false, "the slides' text")],
    },
    Spec {
        name: "add-image",
        help: "Add a picture as a slide (a picture with transparency is an overlay)",
        params: &[p("file", Kind::Text, true, "picture file")],
    },
    Spec { name: "add-video", help: "Add a video (copied into the library)", params: &[p("file", Kind::Text, true, "video file")] },
    Spec { name: "list-items", help: "The items of the service (number, title, kind; * = selected, LIVE = live)", params: &[] },
    Spec {
        name: "select-item",
        help: "Select a service item",
        params: &[p("item", Kind::Text, true, "its number (from 1) or title (or file name of a video)")],
    },
    Spec {
        name: "rename-item",
        help: "Rename the selected service item (F2); empty = its own title",
        params: &[p("name", Kind::Text, false, "the new name")],
    },
    Spec { name: "move-up", help: "Move up (the selected item)", params: &[] },
    Spec { name: "move-down", help: "Move down (the selected item)", params: &[] },
    Spec { name: "duplicate-item", help: "Duplicate (the selected item)", params: &[] },
    Spec { name: "remove-item", help: "Remove from service (the selected item)", params: &[] },
    // Presenting
    Spec { name: "list-slides", help: "The slides of the selected item", params: &[] },
    Spec {
        name: "select-slide",
        help: "Select a slide of the selected item (live mode: it goes live)",
        params: &[p("slide", Kind::Int, true, "its number (from 1)")],
    },
    Spec { name: "next-slide", help: "Next slide", params: &[] },
    Spec { name: "prev-slide", help: "Previous slide", params: &[] },
    Spec { name: "first-slide", help: "First slide", params: &[] },
    Spec { name: "last-slide", help: "Last slide", params: &[] },
    Spec { name: "next-item", help: "Next item", params: &[] },
    Spec { name: "prev-item", help: "Previous item", params: &[] },
    Spec { name: "go-live", help: "Go live (the selected slide)", params: &[] },
    Spec {
        name: "live-mode",
        help: "Live mode: selecting a slide shows it on the output immediately",
        params: &[p("state", ON_OFF, true, "on / off")],
    },
    Spec { name: "blank", help: "Blank screen (toggle, or on / off)", params: &[p("state", ON_OFF, false, "on / off")] },
    Spec { name: "freeze", help: "Freeze output (toggle, or on / off)", params: &[p("state", ON_OFF, false, "on / off")] },
    Spec {
        name: "playback-control",
        help: "Video: play, pause, play-pause, play-loop, play-once, restart, seek (seconds=), forward / back (5 s), mute, unmute. On the live video, else the selected video goes live",
        params: &[p("action", Kind::OneOf(PLAYBACK), true, "what to do"), p("seconds", Kind::Number, false, "position for seek")],
    },
    // Output
    Spec {
        name: "output",
        help: "Output window: open, close, fullscreen, windowed",
        params: &[p("state", Kind::OneOf(&["open", "close", "fullscreen", "windowed"]), true, "what to do")],
    },
    Spec { name: "alert", help: "Alert: a message running across the output", params: &[p("text", Kind::Text, true, "the message")] },
    Spec { name: "stop-alert", help: "Stop the alert", params: &[] },
    // Other tabs
    Spec {
        name: "show-tab",
        help: "Show a tab of the program",
        params: &[p("tab", Kind::OneOf(&["service", "songs", "bible", "settings"]), true, "the tab")],
    },
    Spec { name: "list-songs", help: "Songs of the library", params: &[p("search", Kind::Text, false, "only titles containing this")] },
    Spec { name: "list-designs", help: "Designs of the library", params: &[] },
    Spec { name: "list-bibles", help: "Bibles (id, name; * = current)", params: &[] },
    Spec {
        name: "bible",
        help: "Bible: search (reference like \"John 3:16\" or words) or show (live the found verses, as Show live)",
        params: &[
            p("action", Kind::OneOf(&["search", "show", "add"]), true, "search / show / add (to the service)"),
            p("bible", Kind::Text, false, "bible id or name (default: the current one)"),
            p("query", Kind::Text, false, "reference or words (search)"),
        ],
    },
    // State and settings
    Spec { name: "status", help: "What is live, selected, blanked, frozen, the output window", params: &[] },
    Spec { name: "get-settings", help: "All settings (JSON, as in Settings)", params: &[] },
    Spec {
        name: "set-setting",
        help: "Change a setting (as in Settings): key = a name from get-settings (a.b for nested), value = JSON",
        params: &[p("key", Kind::Text, true, "setting name"), p("value", Kind::Text, true, "new value as JSON (e.g. true, 3, \"cs\")")],
    },
];

pub fn spec(name: &str) -> Option<&'static Spec> {
    COMMANDS.iter().find(|s| s.name == name)
}

/// Splits a command line into words: spaces separate them, "double" or 'single' quotes
/// keep spaces inside one, `\` takes the next character as it is.
pub fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (_, '\\') => {
                cur.push(chars.next().ok_or("a \\ at the end of the line")?);
                in_word = true;
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("a quote is not closed".into());
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

/// Turns a typed value into JSON of the parameter's kind.
fn value_of(param: &Param, text: &str) -> Result<Value, String> {
    let bad = |what: &str| format!("{}: {what}, not {text:?}", param.name);
    match param.kind {
        Kind::Text => Ok(Value::String(text.to_string())),
        Kind::Int => text.parse::<i64>().map(Value::from).map_err(|_| bad("a whole number")),
        Kind::Number => {
            text.parse::<f64>().ok().and_then(|n| serde_json::Number::from_f64(n).map(Value::Number)).ok_or_else(|| bad("a number"))
        }
        Kind::Bool => match text.to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err(bad("true or false")),
        },
        Kind::OneOf(words) => {
            if words.contains(&text) {
                Ok(Value::String(text.to_string()))
            } else {
                Err(bad(&format!("one of {}", words.join(", "))))
            }
        }
    }
}

/// A command line (`select-item 3`, `add-text Welcome "Good morning"`, `alert The car 1AB…`):
/// the command and its arguments. Words go to the parameters in order — the required ones,
/// the last one, and optional ones that are not text (an optional text parameter that is not
/// the last one is set by name: `bible=KJV`); the last text parameter takes the rest of the
/// line. `name=value` sets any parameter by name.
pub fn parse_line(line: &str) -> Result<(String, Map<String, Value>), String> {
    let words = split_words(line)?;
    let Some((name, rest)) = words.split_first() else { return Err("empty command".into()) };
    let spec = spec(name).ok_or_else(|| format!("unknown command {name:?} (help lists them)"))?;
    let last = spec.params.last().map(|p| p.name);
    let positional: Vec<&Param> = spec.params.iter().filter(|p| p.required || Some(p.name) == last || p.kind != Kind::Text).collect();
    let mut args = Map::new();
    let mut rest = rest.iter();
    while let Some(w) = rest.next() {
        if let Some((k, v)) = w.split_once('=')
            && let Some(param) = spec.params.iter().find(|p| p.name == k)
        {
            args.insert(k.to_string(), value_of(param, v)?);
            continue;
        }
        let param = positional.iter().find(|p| !args.contains_key(p.name)).ok_or_else(|| format!("{name}: too many arguments"))?;
        let text = if param.kind == Kind::Text && Some(param.name) == last {
            std::iter::once(w.clone()).chain(rest.by_ref().cloned()).collect::<Vec<_>>().join(" ")
        } else {
            w.clone()
        };
        args.insert(param.name.to_string(), value_of(param, &text)?);
    }
    check_args(spec, &args)?;
    Ok((name.to_string(), args))
}

/// Checks JSON arguments (MCP) against the command: known names, right kinds, the required
/// ones there.
pub fn check_args(spec: &Spec, args: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in args {
        let param = spec.params.iter().find(|p| p.name == k).ok_or_else(|| format!("{}: unknown argument {k:?}", spec.name))?;
        let ok = match param.kind {
            Kind::Text => v.is_string(),
            Kind::Int => v.is_i64() || v.is_u64(),
            Kind::Number => v.is_number(),
            Kind::Bool => v.is_boolean(),
            Kind::OneOf(words) => v.as_str().is_some_and(|s| words.contains(&s)),
        };
        if !ok {
            return Err(format!("{}: wrong value of {k}: {v}", spec.name));
        }
    }
    if let Some(missing) = spec.params.iter().find(|p| p.required && !args.contains_key(p.name)) {
        return Err(format!("{}: {} is missing ({})", spec.name, missing.name, missing.help));
    }
    Ok(())
}

/// JSON schema of a command's arguments (MCP `inputSchema`).
pub fn input_schema(spec: &Spec) -> Value {
    let mut props = Map::new();
    for p in spec.params {
        let mut s = match p.kind {
            Kind::Text => json!({"type": "string"}),
            Kind::Int => json!({"type": "integer"}),
            Kind::Number => json!({"type": "number"}),
            Kind::Bool => json!({"type": "boolean"}),
            Kind::OneOf(words) => json!({"type": "string", "enum": words}),
        };
        s["description"] = Value::String(p.help.to_string());
        props.insert(p.name.to_string(), s);
    }
    let required: Vec<&str> = spec.params.iter().filter(|p| p.required).map(|p| p.name).collect();
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

/// The commands, for `help` on the command line.
pub fn help_text() -> String {
    let mut out = String::new();
    for s in COMMANDS {
        let params: Vec<String> =
            s.params.iter().map(|p| if p.required { format!("<{}>", p.name) } else { format!("[{}]", p.name) }).collect();
        out.push_str(&format!("  {:<18} {:<26} {}\n", s.name, params.join(" "), s.help));
    }
    out.push_str(
        "  help                                         this list\n  quit                                         end the program\n",
    );
    out
}

/// A command sent to the UI thread, with where its answer goes.
pub struct Request {
    pub name: String,
    pub args: Map<String, Value>,
    /// After this the sender no longer waits: the command is dropped, not run late.
    pub deadline: std::time::Instant,
    pub reply: mpsc::Sender<Result<String, String>>,
}

/// Sends commands to the UI thread and wakes it up.
#[derive(Clone)]
pub struct Sender {
    tx: mpsc::Sender<Request>,
    ctx: eframe::egui::Context,
}

impl Sender {
    pub fn new(tx: mpsc::Sender<Request>, ctx: eframe::egui::Context) -> Self {
        Self { tx, ctx }
    }

    /// Runs a command in the UI thread and waits for its answer (at most `timeout`).
    pub fn call(&self, name: &str, args: Map<String, Value>, timeout: std::time::Duration) -> Result<String, String> {
        let (reply, answer) = mpsc::channel();
        let deadline = std::time::Instant::now() + timeout;
        self.tx.send(Request { name: name.to_string(), args, reply, deadline }).map_err(|_| "the program is ending".to_string())?;
        self.ctx.request_repaint();
        answer.recv_timeout(timeout).map_err(|_| "the program did not answer in time (is its window minimized?)".to_string())?
    }
}

/// Reads commands from stdin (`--cli`); each answer is printed. `quit` ends the program; the
/// end of stdin does not (started without a terminal, e.g. from a launcher, stdin is empty
/// and the output window keeps running until it is closed).
pub fn stdin_loop(sender: Sender) {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line {
            "help" | "?" => {
                print!("{}", help_text());
                continue;
            }
            "quit" | "exit" => {
                let _ = sender.call("quit", Map::new(), std::time::Duration::from_secs(5));
                return;
            }
            _ => {}
        }
        match parse_line(line).and_then(|(name, args)| sender.call(&name, args, std::time::Duration::from_secs(30))) {
            Ok(out) => println!("{out}"),
            Err(e) => eprintln!("error: {e}"),
        }
    }
}
