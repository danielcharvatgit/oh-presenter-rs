//! Keeps the desktop off the projector while the output is full screen on another screen.
//!
//! Some keys of the desktop show other windows or the desktop on every screen, so also on the
//! projector, in front of everybody:
//!
//! * **GNOME** (also Ubuntu, Unity, Pop!_OS…): Super opens the Activities overview on all
//!   screens — mutter's `overlay-key` is cleared while presenting;
//! * **Cinnamon**: Super opens the menu / Expo — muffin's `overlay-key` is cleared;
//! * **KDE Plasma**: Meta alone opens the launcher / overview — kwin's
//!   `ModifierOnlyShortcuts / Meta` is cleared (kwin is told to reload its settings);
//! * **Hyprland** and other window managers: their key binds are the user's own and are not
//!   changed; the output window is put back full screen when a key takes it out
//!   (see `keep_output_fullscreen` in the app), on X11 and Wayland alike;
//! * **Windows**: Win+Tab (Task View), Win+D (desktop), Win+M (minimize all) and Win+,
//!   (peek at the desktop) are swallowed by a low-level keyboard hook.
//!
//! The changed desktop settings are saved before they are changed and put back when the
//! output leaves full screen, closes, or the program quits — and at the next start when the
//! program did not get to it (a crash, a power cut).

use serde::{Deserialize, Serialize};

/// The desktop the program runs on (Linux), from `XDG_CURRENT_DESKTOP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Desktop {
    Gnome,
    Cinnamon,
    Plasma,
    Hyprland,
    Other,
}

impl Desktop {
    /// Reads `XDG_CURRENT_DESKTOP` (a `:` separated list, e.g. `ubuntu:GNOME`).
    pub fn from_xdg(value: &str) -> Self {
        let names: Vec<String> = value.split(':').map(|s| s.trim().to_ascii_lowercase()).collect();
        let has = |n: &str| names.iter().any(|x| x == n);
        if has("x-cinnamon") || has("cinnamon") {
            Desktop::Cinnamon
        } else if has("kde") || has("plasma") {
            Desktop::Plasma
        } else if has("hyprland") {
            Desktop::Hyprland
        } else if has("gnome") || has("unity") || has("ubuntu") || has("pop") || has("gnome-classic") || has("gnome-flashback") {
            Desktop::Gnome
        } else {
            Desktop::Other
        }
    }

    pub fn current() -> Self {
        Self::from_xdg(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
    }
}

/// One desktop setting the guard turns off: how to read it, set it, and put it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    /// Reads the current value (its output, trimmed, is the value).
    pub read: Vec<String>,
    /// Sets a value: `{}` in the arguments is replaced by it.
    pub write: Vec<String>,
    /// Removes the setting (back to the desktop's default), used when it was not set before.
    pub delete: Option<Vec<String>>,
    /// The value that turns the key off.
    pub off: String,
    /// Run after a write (tells the desktop to reload).
    pub after: Option<Vec<String>>,
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// The settings to turn off on `desktop` (empty: nothing is changed there).
pub fn settings_for(desktop: Desktop) -> Vec<Setting> {
    let gsettings = |schema: &str| Setting {
        read: args(&["gsettings", "get", schema, "overlay-key"]),
        write: args(&["gsettings", "set", schema, "overlay-key", "{}"]),
        delete: None,
        // gsettings prints strings quoted ('Super_L'); setting the printed form back works
        off: "''".into(),
        after: None,
    };
    match desktop {
        Desktop::Gnome => vec![gsettings("org.gnome.mutter")],
        Desktop::Cinnamon => vec![gsettings("org.cinnamon.muffin")],
        Desktop::Plasma => {
            let kwin = ["--file", "kwinrc", "--group", "ModifierOnlyShortcuts", "--key", "Meta"];
            let mut read = args(&["kreadconfig6"]);
            read.extend(args(&kwin));
            let mut write = args(&["kwriteconfig6"]);
            write.extend(args(&kwin));
            write.push("{}".into());
            let mut delete = args(&["kwriteconfig6"]);
            delete.extend(args(&kwin));
            delete.push("--delete".into());
            Setting {
                read,
                write,
                delete: Some(delete),
                off: String::new(),
                after: Some(args(&[
                    "dbus-send",
                    "--session",
                    "--type=method_call",
                    "--dest=org.kde.KWin",
                    "/KWin",
                    "org.kde.KWin.reconfigure",
                ])),
            }
            .into_vec()
        }
        Desktop::Hyprland | Desktop::Other => Vec::new(),
    }
}

impl Setting {
    fn into_vec(self) -> Vec<Setting> {
        vec![self]
    }

    /// The command that sets `value`.
    pub fn write_cmd(&self, value: &str) -> Vec<String> {
        self.write.iter().map(|a| if a == "{}" { value.to_string() } else { a.clone() }).collect()
    }

    /// The commands that put `saved` back (`None`: it was not set, so it is removed when
    /// that is possible).
    pub fn restore_cmds(&self, saved: Option<&str>) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        match (saved, &self.delete) {
            (Some(v), _) => out.push(self.write_cmd(v)),
            (None, Some(d)) => out.push(d.clone()),
            (None, None) => {}
        }
        if !out.is_empty()
            && let Some(a) = &self.after
        {
            out.push(a.clone());
        }
        out
    }
}

/// Runs a command (through `flatpak-spawn --host` inside a flatpak) and returns its output.
pub type Runner = fn(&[String]) -> Option<String>;

/// Runs commands on this computer.
pub fn run_host(cmd: &[String]) -> Option<String> {
    let mut full: Vec<String> = Vec::new();
    if std::path::Path::new("/.flatpak-info").exists() {
        full.extend(args(&["flatpak-spawn", "--host"]));
    }
    full.extend(cmd.iter().cloned());
    let (prog, rest) = full.split_first()?;
    let out = std::process::Command::new(prog).args(rest).stdin(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What was changed, saved in the config folder until it is put back.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub desktop: Option<Desktop>,
    /// The value of each setting before the change (`None` = it was not set).
    pub values: Vec<Option<String>>,
    /// Which settings were changed (one that could not be read is left alone, so it can
    /// always be put back).
    #[serde(default)]
    pub changed: Vec<bool>,
}

/// Turns the desktop keys off and on again (one per program).
pub struct Guard {
    desktop: Desktop,
    run: Runner,
    file: std::path::PathBuf,
    engaged: bool,
}

impl Guard {
    pub fn new(desktop: Desktop, run: Runner, file: std::path::PathBuf) -> Self {
        let mut g = Self { desktop, run, file, engaged: false };
        // left over from a run that did not get to put them back
        g.restore_saved();
        g
    }

    /// For this computer: its desktop, real commands, the file in the config folder.
    pub fn for_this_computer() -> Self {
        // tests never touch the desktop of the computer they run on
        let run: Runner = if cfg!(test) { |_| Some(String::new()) } else { run_host };
        Self::new(Desktop::current(), run, crate::storage::paths().config.join("desktop-guard.json"))
    }

    #[cfg(test)]
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// Engages (`on`) or releases the guard; nothing happens when it is in that state already.
    pub fn set(&mut self, on: bool) {
        if on == self.engaged {
            return;
        }
        self.engaged = on;
        #[cfg(windows)]
        windows::set_active(on);
        if on {
            self.engage()
        } else {
            self.restore_saved();
        }
    }

    fn engage(&mut self) {
        let settings = settings_for(self.desktop);
        if settings.is_empty() {
            return;
        }
        // values saved before and not put back yet are the user's own: never saved over
        if self.file.exists() && !self.restore_saved() {
            crate::log_error("desktop keys: the saved settings could not be put back yet, so they are not changed again");
            return;
        }
        let read: Vec<Option<String>> = settings.iter().map(|s| (self.run)(&s.read)).collect();
        let changed: Vec<bool> = read.iter().map(Option::is_some).collect();
        if !changed.contains(&true) {
            crate::log_info(&format!("desktop keys of {:?}: settings could not be read, not changed", self.desktop));
            return;
        }
        let values = read.into_iter().map(|v| v.filter(|v| !v.is_empty())).collect();
        let saved = Saved { desktop: Some(self.desktop), values, changed: changed.clone() };
        // saved first: a crash right after the change still lets the next start put it back
        if let Err(e) = crate::storage::write_atomic(&self.file, &serde_json::to_vec_pretty(&saved).unwrap_or_default(), false) {
            crate::log_error(&format!("desktop keys: could not save the settings, not changed: {e}"));
            return;
        }
        for (s, _) in settings.iter().zip(&changed).filter(|(_, c)| **c) {
            (self.run)(&s.write_cmd(&s.off));
            if let Some(a) = &s.after {
                (self.run)(a);
            }
        }
        crate::log_info(&format!("desktop keys of {:?} turned off while presenting", self.desktop));
    }

    /// Puts back what the saved file says was changed, then removes the file. When a
    /// command fails the file stays (with what is still to be put back), so the next try (the
    /// next release, the next start) does it: `false` then.
    fn restore_saved(&mut self) -> bool {
        let Ok(bytes) = std::fs::read(&self.file) else { return true };
        let Ok(mut saved) = serde_json::from_slice::<Saved>(&bytes) else {
            crate::log_error("desktop keys: the saved settings file is damaged, removed");
            let _ = std::fs::remove_file(&self.file);
            return true;
        };
        let desktop = saved.desktop.unwrap_or(self.desktop);
        let mut all = true;
        for (i, (s, v)) in settings_for(desktop).iter().zip(saved.values.clone()).enumerate() {
            if !saved.changed.get(i).copied().unwrap_or(true) {
                continue;
            }
            let ok = s.restore_cmds(v.as_deref()).iter().all(|cmd| (self.run)(cmd).is_some());
            if ok {
                if let Some(c) = saved.changed.get_mut(i) {
                    *c = false;
                } else {
                    saved.changed.resize(i + 1, true);
                    saved.changed[i] = false;
                }
            } else {
                all = false;
            }
        }
        if all {
            let _ = std::fs::remove_file(&self.file);
            crate::log_info(&format!("desktop keys of {desktop:?} put back"));
        } else {
            let _ = crate::storage::write_atomic(&self.file, &serde_json::to_vec_pretty(&saved).unwrap_or_default(), false);
            crate::log_error(&format!("desktop keys of {desktop:?}: could not all be put back, tried again later"));
        }
        all
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.set(false);
    }
}

/// Windows: a low-level keyboard hook that swallows Win+Tab, Win+D, Win+M and Win+, while
/// active. It runs on its own thread (a hook needs a message loop there).
#[cfg(windows)]
pub mod windows {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static INSTALLED: std::sync::Once = std::sync::Once::new();

    const WH_KEYBOARD_LL: i32 = 13;
    const WM_KEYDOWN: usize = 0x0100;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const VK_LWIN: i32 = 0x5B;
    const VK_RWIN: i32 = 0x5C;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    /// An unassigned key: sent so that releasing Win does not open the Start menu.
    const VK_UNASSIGNED: u8 = 0xE8;

    #[repr(C)]
    struct KbdLlHookStruct {
        vk_code: u32,
        scan_code: u32,
        flags: u32,
        time: u32,
        extra_info: usize,
    }

    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: [i32; 2],
        private: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowsHookExW(
            id: i32,
            proc_: unsafe extern "system" fn(i32, usize, isize) -> isize,
            hmod: *mut c_void,
            thread: u32,
        ) -> *mut c_void;
        fn CallNextHookEx(hook: *mut c_void, code: i32, wparam: usize, lparam: isize) -> isize;
        fn GetMessageW(msg: *mut Msg, hwnd: *mut c_void, min: u32, max: u32) -> i32;
        fn GetAsyncKeyState(vk: i32) -> i16;
        fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
    }

    /// The virtual keys swallowed together with Win: Tab, D, M and ",".
    pub fn blocked(vk: u32) -> bool {
        matches!(vk, 0x09 | 0x44 | 0x4D | 0xBC)
    }

    unsafe extern "system" fn hook(code: i32, wparam: usize, lparam: isize) -> isize {
        if code >= 0 && ACTIVE.load(Ordering::Relaxed) && (wparam == WM_KEYDOWN || wparam == WM_SYSKEYDOWN) {
            // SAFETY: for WH_KEYBOARD_LL, lparam points to a KBDLLHOOKSTRUCT
            let k = unsafe { &*(lparam as *const KbdLlHookStruct) };
            // SAFETY: plain Win32 calls
            let win = unsafe { GetAsyncKeyState(VK_LWIN) < 0 || GetAsyncKeyState(VK_RWIN) < 0 };
            if win && blocked(k.vk_code) {
                unsafe {
                    keybd_event(VK_UNASSIGNED, 0, 0, 0);
                    keybd_event(VK_UNASSIGNED, 0, KEYEVENTF_KEYUP, 0);
                }
                return 1;
            }
        }
        // SAFETY: passes the event on, as every hook must
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    pub fn set_active(on: bool) {
        ACTIVE.store(on, Ordering::Relaxed);
        if on {
            INSTALLED.call_once(|| {
                std::thread::spawn(|| {
                    // SAFETY: installs the hook for this thread's message loop below
                    let h = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, hook, std::ptr::null_mut(), 0) };
                    if h.is_null() {
                        crate::log_error("desktop keys: the keyboard hook could not be installed");
                        return;
                    }
                    let mut msg = std::mem::MaybeUninit::<Msg>::zeroed();
                    // SAFETY: the message loop the hook needs; ends when the program does
                    while unsafe { GetMessageW(msg.as_mut_ptr(), std::ptr::null_mut(), 0, 0) } > 0 {}
                });
            });
        }
    }
}
