//! Keeps the computer awake while the program runs (Settings › General): no sleep, no
//! blanking or locking of the screen in the middle of a service.
//!
//! * **Linux**: the desktop portal's `org.freedesktop.portal.Inhibit` (idle + suspend; works
//!   in a flatpak and on GNOME, KDE, …), else `org.freedesktop.ScreenSaver.Inhibit`;
//! * **Windows**: `SetThreadExecutionState` (display and system required).
//!
//! The request lives on a thread of its own: D-Bus may take a while to answer, and on
//! Windows the state belongs to the thread that set it. Dropping the [`Inhibitor`] lets the
//! computer sleep again. A failure is written to start.log, it never stops the program.

use std::sync::mpsc;

/// Why the program keeps the computer awake (shown by some desktops).
#[cfg(target_os = "linux")]
pub const REASON: &str = "Presenting (oh! Presenter)";

/// Flags of the portal's Inhibit: 4 = suspend, 8 = idle (blanking, locking).
#[cfg(any(target_os = "linux", test))]
pub const PORTAL_FLAGS: u32 = 4 | 8;

/// While this lives, the computer stays awake.
pub struct Inhibitor {
    /// Dropped = the thread lets go.
    _stop: mpsc::Sender<()>,
}

impl Inhibitor {
    pub fn start() -> Self {
        let (stop, wait) = mpsc::channel::<()>();
        let spawned = std::thread::Builder::new().name("keep-awake".into()).spawn(move || {
            let held = platform::inhibit();
            match &held {
                Ok(how) => crate::log_info(&format!("keep awake: {how}")),
                Err(e) => crate::log_info(&format!("keep awake: not possible ({e})")),
            }
            // until the inhibitor is dropped
            let _ = wait.recv();
            if let Ok(how) = held {
                platform::release(how);
            }
        });
        if let Err(e) = spawned {
            crate::log_info(&format!("keep awake: no thread ({e})"));
        }
        Self { _stop: stop }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;
    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedObjectPath, Value};

    /// What holds the computer awake (the connection must stay open).
    pub enum Held {
        Portal(Connection, OwnedObjectPath),
        ScreenSaver(Connection, &'static str, u32),
    }

    impl std::fmt::Display for Held {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Held::Portal(..) => f.write_str("desktop portal (idle + suspend)"),
                Held::ScreenSaver(_, path, _) => write!(f, "org.freedesktop.ScreenSaver at {path}"),
            }
        }
    }

    pub fn inhibit() -> Result<Held, String> {
        let conn = Connection::session().map_err(|e| e.to_string())?;
        let options: HashMap<&str, Value> = HashMap::from([("reason", Value::from(super::REASON))]);
        let portal = conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Inhibit"),
                "Inhibit",
                &("", super::PORTAL_FLAGS, options),
            )
            .and_then(|m| m.body().deserialize::<OwnedObjectPath>());
        let portal_err = match portal {
            Ok(handle) => return Ok(Held::Portal(conn, handle)),
            Err(e) => e.to_string(),
        };
        for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
            let cookie = conn
                .call_method(
                    Some("org.freedesktop.ScreenSaver"),
                    path,
                    Some("org.freedesktop.ScreenSaver"),
                    "Inhibit",
                    &("oh-presenter-rs", super::REASON),
                )
                .and_then(|m| m.body().deserialize::<u32>());
            if let Ok(cookie) = cookie {
                return Ok(Held::ScreenSaver(conn, path, cookie));
            }
        }
        Err(format!("portal: {portal_err}; org.freedesktop.ScreenSaver: not available"))
    }

    pub fn release(held: Held) {
        // closing the connection ends both kinds as well; this says it right away
        match held {
            Held::Portal(conn, handle) => {
                let _ = conn.call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    handle.as_str(),
                    Some("org.freedesktop.portal.Request"),
                    "Close",
                    &(),
                );
            }
            Held::ScreenSaver(conn, path, cookie) => {
                let _ = conn.call_method(
                    Some("org.freedesktop.ScreenSaver"),
                    path,
                    Some("org.freedesktop.ScreenSaver"),
                    "UnInhibit",
                    &(cookie,),
                );
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }

    pub struct Held;

    impl std::fmt::Display for Held {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("SetThreadExecutionState (display + system required)")
        }
    }

    pub fn inhibit() -> Result<Held, String> {
        // SAFETY: a plain Win32 call without pointers
        let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) };
        if previous == 0 { Err("SetThreadExecutionState failed".into()) } else { Ok(Held) }
    }

    pub fn release(_held: Held) {
        // SAFETY: as above
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    pub struct Held;

    impl std::fmt::Display for Held {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("-")
        }
    }

    pub fn inhibit() -> Result<Held, String> {
        Err("not supported on this system".into())
    }

    pub fn release(_held: Held) {}
}
