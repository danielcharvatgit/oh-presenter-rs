//! The tools code agents use to control the page (WebMCP, `navigator.modelContext`; also
//! `window.ohPresenter.call(name, args)`). They are named after the buttons, as the
//! program's command line and MCP tools are, and run as if the operator pressed the button
//! (see `WebApp::run_command`). The web edition has no command line.

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Int,
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
pub const TABS: &[&str] = &["service", "songs", "bible", "settings"];

pub const COMMANDS: &[Spec] = &[
    // Service
    Spec { name: "new-service", help: "New service (the items are removed)", params: &[] },
    Spec {
        name: "add-song",
        help: "Add a song of the library to the service (after the selected item)",
        params: &[p("title", Kind::Text, true, "song title (or its beginning)")],
    },
    Spec {
        name: "add-text",
        help: "Add a text item (a line with only --- starts a new slide)",
        params: &[p("title", Kind::Text, true, "title"), p("text", Kind::Text, false, "the slides' text")],
    },
    Spec {
        name: "add-to-service",
        help: "Add to service of the open tab (key A, the tab's Add to service button): Songs the open song, Bible the selected verses (all found when none is selected), Service the quick passage",
        params: &[p("tab", Kind::OneOf(&["service", "songs", "bible"]), false, "open this tab first (as show-tab)")],
    },
    Spec { name: "list-items", help: "The items of the service (number, title; * = selected, LIVE = live)", params: &[] },
    Spec { name: "select-item", help: "Select a service item", params: &[p("item", Kind::Text, true, "its number (from 1) or title")] },
    Spec {
        name: "rename-item",
        help: "Rename the selected item; empty = its own title",
        params: &[p("name", Kind::Text, false, "the new name")],
    },
    Spec { name: "move-up", help: "Move up (the selected item)", params: &[] },
    Spec { name: "move-down", help: "Move down (the selected item)", params: &[] },
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
    Spec { name: "live-mode", help: "Live mode: selecting a slide shows it immediately", params: &[p("state", ON_OFF, true, "on / off")] },
    Spec { name: "blank", help: "Blank screen (toggle, or on / off)", params: &[p("state", ON_OFF, false, "on / off")] },
    Spec { name: "freeze", help: "Freeze output (toggle, or on / off)", params: &[p("state", ON_OFF, false, "on / off")] },
    // Output
    Spec {
        name: "output",
        help: "Output window: open or close (a browser may block opening without a click on the page)",
        params: &[p("state", Kind::OneOf(&["open", "close"]), true, "open / close")],
    },
    Spec { name: "alert", help: "Alert: a message running across the output", params: &[p("text", Kind::Text, true, "the message")] },
    Spec { name: "stop-alert", help: "Stop the alert", params: &[] },
    // Tabs
    Spec { name: "show-tab", help: "Show a tab", params: &[p("tab", Kind::OneOf(TABS), true, "the tab")] },
    Spec { name: "list-songs", help: "Songs of the library", params: &[p("search", Kind::Text, false, "only titles containing this")] },
    Spec { name: "list-bibles", help: "Bibles (id, name; * = current)", params: &[] },
    Spec {
        name: "bible",
        help: "Bible tab: search (a reference like \"John 3:16\" / \"Jan 3,16-18\", or words), show (the found verses live) or add (to the service)",
        params: &[
            p("action", Kind::OneOf(&["search", "show", "add"]), true, "search / show / add"),
            p("query", Kind::Text, false, "reference or words"),
            p("bible", Kind::Text, false, "bible id, name or abbreviation (default: the current one)"),
        ],
    },
    // State and settings
    Spec { name: "status", help: "What is live, selected, blanked, frozen, the output window", params: &[] },
    Spec { name: "get-settings", help: "All settings (JSON, as in Settings)", params: &[] },
    Spec {
        name: "set-setting",
        help: "Change a setting: key = a name from get-settings (a.b for nested), value = JSON",
        params: &[p("key", Kind::Text, true, "setting name"), p("value", Kind::Text, true, "new value as JSON (e.g. true, 3, \"cs\")")],
    },
];

pub fn spec(name: &str) -> Option<&'static Spec> {
    COMMANDS.iter().find(|s| s.name == name)
}

fn schema(kind: Kind) -> Value {
    match kind {
        Kind::Text => json!({"type": "string"}),
        Kind::Int => json!({"type": "integer"}),
        Kind::OneOf(words) => json!({"type": "string", "enum": words}),
    }
}

/// The tools as WebMCP wants them: `[{name, description, inputSchema}]`.
pub fn tools_json() -> String {
    let tools: Vec<Value> = COMMANDS
        .iter()
        .map(|s| {
            let props: serde_json::Map<String, Value> = s
                .params
                .iter()
                .map(|p| {
                    let mut v = schema(p.kind);
                    v["description"] = json!(p.help);
                    (p.name.to_string(), v)
                })
                .collect();
            let required: Vec<&str> = s.params.iter().filter(|p| p.required).map(|p| p.name).collect();
            json!({
                "name": s.name,
                "description": s.help,
                "inputSchema": {"type": "object", "properties": props, "required": required},
            })
        })
        .collect();
    serde_json::to_string(&tools).unwrap()
}

/// Checks the arguments of a tool call (types, required ones, known names).
pub fn check(name: &str, args: &serde_json::Map<String, Value>) -> Result<&'static Spec, String> {
    let spec = spec(name).ok_or_else(|| format!("unknown command {name:?}"))?;
    for (k, v) in args {
        let p = spec.params.iter().find(|p| p.name == k).ok_or_else(|| format!("{name}: no parameter {k:?}"))?;
        let ok = match p.kind {
            Kind::Text => v.is_string(),
            Kind::Int => v.is_i64() || v.is_u64(),
            Kind::OneOf(words) => v.as_str().is_some_and(|s| words.contains(&s)),
        };
        if !ok {
            return Err(format!(
                "{name}: {k} must be {}",
                match p.kind {
                    Kind::Text => "a text".to_string(),
                    Kind::Int => "a whole number".to_string(),
                    Kind::OneOf(w) => format!("one of {}", w.join(", ")),
                }
            ));
        }
    }
    if let Some(p) = spec.params.iter().find(|p| p.required && !args.contains_key(p.name)) {
        return Err(format!("{name}: {} is required", p.name));
    }
    Ok(spec)
}
