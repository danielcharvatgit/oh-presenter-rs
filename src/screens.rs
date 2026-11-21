//! Which screen gets which window: the program on the primary screen, the output
//! (projector) window full screen on a second one.
//!
//! The monitors come from winit (`Window::available_monitors`, in that order, so an
//! index here is the index `ViewportBuilder::with_monitor` / `ViewportCommand::SetMonitor`
//! expect). Wayland has no primary monitor: there the screen the main window opened on
//! counts as the primary one.

/// One screen, in physical pixels of the virtual desktop.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub pos: (i32, i32),
    pub size: (u32, u32),
    pub scale: f64,
    pub primary: bool,
}

impl Monitor {
    fn overlaps(&self, other: &Monitor) -> bool {
        let (ax, ay) = (self.pos.0 as i64, self.pos.1 as i64);
        let (bx, by) = (other.pos.0 as i64, other.pos.1 as i64);
        let (aw, ah) = (self.size.0 as i64, self.size.1 as i64);
        let (bw, bh) = (other.size.0 as i64, other.size.1 as i64);
        ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
    }
}

/// Where the windows go. `None` = leave it to the system (unknown screens, one screen).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Placement {
    pub main: Option<usize>,
    pub output: Option<usize>,
}

/// `current`: the monitor the main window is on now (if known).
pub fn plan(monitors: &[Monitor], current: Option<usize>) -> Placement {
    if monitors.is_empty() {
        return Placement::default();
    }
    let main = monitors.iter().position(|m| m.primary).or(current.filter(|&c| c < monitors.len())).unwrap_or(0);
    // the first other screen that really is another one: a mirrored screen (the same
    // picture as the main one, e.g. a projector in "duplicate" mode) shares its area,
    // and a full-screen output there would hide the program
    let output = (0..monitors.len()).find(|&i| i != main && !monitors[i].overlaps(&monitors[main]));
    Placement { main: Some(main), output }
}

/// Whether [`plan`] would have to guess the program's screen: several screens, none
/// primary (Wayland) and the main window's screen not known yet (it is known only once the
/// window is shown there).
pub fn must_wait(monitors: &[Monitor], current: Option<usize>) -> bool {
    monitors.len() > 1 && !monitors.iter().any(|m| m.primary) && current.is_none_or(|c| c >= monitors.len())
}

/// Top-left position (in physical pixels) that centres a window of `size` physical
/// pixels on `m`, kept inside the screen when the window is bigger than it.
pub fn centered_on(m: &Monitor, size: (u32, u32)) -> (i32, i32) {
    let free_w = (m.size.0 as i64 - size.0 as i64).max(0) / 2;
    let free_h = (m.size.1 as i64 - size.1 as i64).max(0) / 2;
    (m.pos.0 + free_w as i32, m.pos.1 + free_h as i32)
}

/// Whether the point (physical pixels) lies on `m`.
pub fn contains(m: &Monitor, p: (i32, i32)) -> bool {
    let (x, y) = (p.0 as i64, p.1 as i64);
    let (mx, my) = (m.pos.0 as i64, m.pos.1 as i64);
    x >= mx && y >= my && x < mx + m.size.0 as i64 && y < my + m.size.1 as i64
}

/// The screens as winit reports them, and the index of the one `window` is on.
pub fn detect(window: &winit::window::Window) -> (Vec<Monitor>, Option<usize>) {
    let primary = window.primary_monitor();
    let current = window.current_monitor();
    let mut out = Vec::new();
    let mut current_index = None;
    for (i, m) in window.available_monitors().enumerate() {
        if current.as_ref() == Some(&m) {
            current_index = Some(i);
        }
        let pos = m.position();
        let size = m.size();
        out.push(Monitor {
            name: m.name().unwrap_or_default(),
            pos: (pos.x, pos.y),
            size: (size.width, size.height),
            scale: m.scale_factor(),
            primary: primary.as_ref() == Some(&m),
        });
    }
    (out, current_index)
}

// ---------------------------------------------------------------------------
// The output's screen and getting the output window full screen on it

/// A screen remembered between starts (the one the output was last shown on).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenId {
    pub name: String,
    pub pos: (i32, i32),
    pub size: (u32, u32),
}

impl ScreenId {
    pub fn of(m: &Monitor) -> Self {
        Self { name: m.name.clone(), pos: m.pos, size: m.size }
    }

    /// The screen of `monitors` this is: the same name and place, else the same name,
    /// else the same place and size.
    pub fn find(&self, monitors: &[Monitor]) -> Option<usize> {
        let named = |m: &Monitor| !self.name.is_empty() && m.name == self.name;
        monitors
            .iter()
            .position(|m| named(m) && m.pos == self.pos)
            .or_else(|| monitors.iter().position(named))
            .or_else(|| monitors.iter().position(|m| m.pos == self.pos && m.size == self.size))
    }
}

/// The screen for the output window: the one it was last shown on, if it is still there and
/// is not the program's screen; else the first other screen (not a mirror of the program's).
/// `main` = the screen the program window is on now.
pub fn output_screen(monitors: &[Monitor], main: Option<usize>, last: Option<&ScreenId>) -> Option<usize> {
    let main = main.filter(|&m| m < monitors.len()).or_else(|| plan(monitors, None).main)?;
    let other = |i: usize| i != main && !monitors[i].overlaps(&monitors[main]);
    if let Some(i) = last.and_then(|l| l.find(monitors)).filter(|&i| other(i)) {
        return Some(i);
    }
    (0..monitors.len()).find(|&i| other(i))
}

/// The output window as egui sees it, in physical pixels (unknown parts are `None`:
/// Wayland tells no window positions).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Seen {
    /// Outer rect: left, top, width, height.
    pub outer: Option<(i32, i32, u32, u32)>,
    pub inner_size: Option<(u32, u32)>,
    pub fullscreen: Option<bool>,
}

/// One thing to do to the output window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Full screen right on the screen (borderless, on that monitor).
    SetMonitor,
    LeaveFullscreen,
    /// Move the window (outer top-left, physical pixels).
    MoveTo((i32, i32)),
    /// Full screen on the screen the window is on now.
    EnterFullscreen,
}

/// Steps tried before giving up.
pub const MAX_STEPS: u32 = 12;

/// Whether the window's middle is on `m` (`None` = position unknown).
pub fn is_on(m: &Monitor, seen: &Seen) -> Option<bool> {
    seen.outer.map(|(x, y, w, h)| contains(m, (x + w as i32 / 2, y + h as i32 / 2)))
}

fn about(a: u32, b: u32) -> bool {
    a.abs_diff(b) <= (b / 50).max(4)
}

/// Whether the window is full screen on `m` and fills it (what cannot be known counts as right).
pub fn fullscreen_ok(m: &Monitor, seen: &Seen) -> bool {
    seen.fullscreen == Some(true)
        && is_on(m, seen) != Some(false)
        && seen.inner_size.is_none_or(|(w, h)| about(w, m.size.0) && about(h, m.size.1))
}

/// The next step for getting the window full screen on `m`, after `done` steps (None = it is).
///
/// First the direct way (full screen on that monitor). Windows sometimes leaves the window
/// borderless but where and as big as it was (a quarter of the primary screen); then:
/// out of full screen, move the window onto the screen, full screen there — again until it fills it.
pub fn fullscreen_step(m: &Monitor, seen: &Seen, done: u32) -> Option<Step> {
    if fullscreen_ok(m, seen) {
        return None;
    }
    if done == 0 {
        return Some(Step::SetMonitor);
    }
    if seen.fullscreen == Some(true) {
        return Some(Step::LeaveFullscreen);
    }
    if is_on(m, seen) == Some(false) {
        let size = seen.outer.map(|(_, _, w, h)| (w, h)).unwrap_or((m.size.0 / 2, m.size.1 / 2));
        return Some(Step::MoveTo(centered_on(m, size)));
    }
    // on the right screen and windowed: full screen there (now and then the direct way again)
    Some(if done % 4 == 3 { Step::SetMonitor } else { Step::EnterFullscreen })
}

/// The next step for a windowed output that should open on `m` (None = it is fine). Only
/// moves it right after opening (`done == 0`), so a window the user moved stays there.
pub fn windowed_step(m: Option<&Monitor>, seen: &Seen, done: u32) -> Option<Step> {
    if seen.fullscreen == Some(true) {
        return Some(Step::LeaveFullscreen);
    }
    let m = m?;
    if done <= 1 && is_on(m, seen) == Some(false) {
        let (_, _, w, h) = seen.outer?;
        return Some(Step::MoveTo(centered_on(m, (w, h))));
    }
    None
}
