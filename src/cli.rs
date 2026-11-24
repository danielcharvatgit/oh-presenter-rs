//! The command line of the program: `--help`, `--version`, and `--cli` (only the presenting
//! output window, controlled by options and by commands typed on stdin, see [`crate::remote`]).
//!
//! Options are named after the buttons of the program (in English):
//!
//! ```text
//! oh-presenter-rs --cli --service sunday.ohsvc --service-item video.mp4 --playback-control=play-loop
//! ```

use serde_json::{Map, Value};
use std::path::PathBuf;

/// What the command line asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    /// Only the output window and the command line (no program window).
    pub cli: bool,
    /// Open this service file.
    pub service: Option<PathBuf>,
    /// Select this item (number from 1, title or video file name) and show it live.
    pub service_item: Option<String>,
    /// Select this slide of the item (from 1).
    pub slide: Option<u32>,
    /// What to do with the video (`play-loop`, …), see [`crate::remote::PLAYBACK`].
    pub playback_control: Option<String>,
    /// The output window as a window, not full screen.
    pub windowed: bool,
    /// Live mode on / off.
    pub live_mode: Option<bool>,
    /// Start the MCP server (whatever the settings say), on this port if given.
    pub mcp: bool,
    pub mcp_port: Option<u16>,
    /// Commands to run after the start (`--command "next-slide"`, repeatable).
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    /// The presenter (with or without `--cli`).
    Run(Options),
    Help,
    Version,
}

/// The program's name in the help.
pub const PROGRAM: &str = "oh-presenter-rs";

pub fn help() -> String {
    format!(
        "oh! Presenter {version}: songs, bible passages, videos and designed slides on an output window\n\
         \n\
         Usage:\n\
         \x20 {PROGRAM} [options] [SERVICE-FILE]     the presenter (a file alone = --service FILE)\n\
         \x20 {PROGRAM} --cli [options]              only the output window, controlled from this command line\n\
         \x20 {PROGRAM} canvas [host[:port]] [--name NAME]\n\
         \x20                                        an output window that receives slides over the network\n\
         \n\
         Options (named after the buttons of the program):\n\
         \x20 --cli                       only the presenting output window; commands are read from stdin\n\
         \x20 --service FILE              Open service… (.ohsvc, OpenLP .osz / .oszl, old .ohpres)\n\
         \x20 --service-item ITEM         select the item (number from 1, title or video file name) and Go live\n\
         \x20 --slide N                   with --service-item: the slide to show (from 1)\n\
         \x20 --playback-control ACTION   video: {playback}\n\
         \x20 --live-mode on|off          Live mode: selecting a slide shows it on the output immediately\n\
         \x20 --windowed                  the output as a window (default: full screen, on the second screen if there is one)\n\
         \x20 --command \"COMMAND\"         run a command after the start (repeatable), e.g. --command \"alert Welcome\"\n\
         \x20 --mcp                       start the MCP server for code agents (else: Settings › Advanced)\n\
         \x20 --mcp-port PORT             its port (default {port})\n\
         \x20 -h, --help                  this help\n\
         \x20 -V, --version               the version\n\
         \n\
         An option's value can also follow a '=': --playback-control=play-loop\n\
         \n\
         Commands (--cli: type them, one per line; the same tools are offered over MCP):\n\
         {commands}\n\
         Example:\n\
         \x20 {PROGRAM} --cli --service sunday.ohsvc --service-item video.mp4 --playback-control=play-loop\n",
        version = env!("CARGO_PKG_VERSION"),
        playback = crate::remote::PLAYBACK.join(", "),
        port = crate::remote::mcp::DEFAULT_PORT,
        commands = crate::remote::help_text(),
    )
}

/// Reads the command line (without the program's name). `canvas` is handled before this.
pub fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            match &inline {
                Some(v) => Ok(v.clone()),
                None => it.next().cloned().ok_or_else(|| format!("{name} needs {what}")),
            }
        };
        match name {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--cli" => o.cli = true,
            "--service" => o.service = Some(PathBuf::from(value("a service file")?)),
            "--service-item" => o.service_item = Some(value("an item (number or title)")?),
            "--slide" => o.slide = Some(value("a slide number")?.parse().map_err(|_| "--slide needs a number (from 1)".to_string())?),
            "--playback-control" => {
                let v = value("an action")?;
                if !crate::remote::PLAYBACK.contains(&v.as_str()) {
                    return Err(format!("--playback-control: one of {}, not {v:?}", crate::remote::PLAYBACK.join(", ")));
                }
                o.playback_control = Some(v);
            }
            "--live-mode" => {
                o.live_mode = Some(match value("on or off")?.as_str() {
                    "on" => true,
                    "off" => false,
                    v => return Err(format!("--live-mode: on or off, not {v:?}")),
                })
            }
            "--windowed" => o.windowed = true,
            "--command" => {
                let c = value("a command")?;
                crate::remote::parse_line(&c).map_err(|e| format!("--command {c:?}: {e}"))?;
                o.commands.push(c);
            }
            "--mcp" => o.mcp = true,
            "--mcp-port" => {
                o.mcp = true;
                let port: u16 = value("a port")?.parse().map_err(|_| "--mcp-port needs a port number (1024–65535)".to_string())?;
                if port < 1024 {
                    return Err("--mcp-port needs a port number (1024–65535)".into());
                }
                o.mcp_port = Some(port);
            }
            // a file on its own (a file manager's "open with"): Open service
            other if !other.starts_with('-') && o.service.is_none() => o.service = Some(PathBuf::from(other)),
            other => return Err(format!("unknown option {other:?} (--help lists them)")),
        }
    }
    if o.slide.is_some() && o.service_item.is_none() {
        return Err("--slide needs --service-item".into());
    }
    Ok(Parsed::Run(o))
}

impl Options {
    /// The commands the options stand for, run once the library is loaded.
    pub fn startup_commands(&self) -> Vec<(String, Map<String, Value>)> {
        let mut out = Vec::new();
        let cmd = |name: &str, args: &[(&str, Value)]| (name.to_string(), args.iter().map(|(k, v)| (k.to_string(), v.clone())).collect());
        if let Some(on) = self.live_mode {
            out.push(cmd("live-mode", &[("state", Value::from(if on { "on" } else { "off" }))]));
        }
        if let Some(f) = &self.service {
            out.push(cmd("open-service", &[("file", Value::from(f.to_string_lossy().to_string())), ("discard", Value::Bool(true))]));
        }
        if let Some(item) = &self.service_item {
            out.push(cmd("select-item", &[("item", Value::from(item.clone()))]));
            if let Some(n) = self.slide {
                out.push(cmd("select-slide", &[("slide", Value::from(n))]));
            }
            out.push(cmd("go-live", &[]));
        }
        if let Some(p) = &self.playback_control {
            out.push(cmd("playback-control", &[("action", Value::from(p.clone()))]));
        }
        for c in &self.commands {
            if let Ok(parsed) = crate::remote::parse_line(c) {
                out.push(parsed);
            }
        }
        out
    }
}

/// Windows: a program without a console window (as this one) prints nothing in the command
/// prompt it was started from. The console of that prompt is attached, or a new one opened
/// for `--cli` started from Explorer.
#[cfg(windows)]
pub fn attach_console(open_new: bool) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
        fn AllocConsole() -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 calls without pointers
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 && open_new {
            AllocConsole();
        }
    }
}

#[cfg(not(windows))]
pub fn attach_console(_open_new: bool) {}
