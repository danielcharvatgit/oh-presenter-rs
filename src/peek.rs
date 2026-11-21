//! Windows "Peek": when the mouse rests on a window's preview above the taskbar (or on
//! "Show desktop", or in Alt+Tab), Windows hides every other window and shows only that
//! one, on all screens, so the projector would show someone else's program or the desktop.
//! A window marked `DWMWA_EXCLUDED_FROM_PEEK` stays shown: the output window gets it.
//!
//! winit gives no handle of a child (output) window, so the window is found by its title
//! among this program's own windows.

/// Marks this program's windows titled `title` as staying shown during Peek; `true` when
/// one was found. Nothing on other systems.
#[cfg(windows)]
pub fn keep_shown(title: &str) -> bool {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, param: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32) -> i32;
    }
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *const c_void, size: u32) -> i32;
    }
    const DWMWA_EXCLUDED_FROM_PEEK: u32 = 12;

    struct Search {
        pid: u32,
        title: Vec<u16>,
        found: Vec<Hwnd>,
    }

    unsafe extern "system" fn visit(hwnd: Hwnd, param: isize) -> i32 {
        // SAFETY: `param` is the `Search` passed to EnumWindows below, alive during the call
        let search = unsafe { &mut *(param as *mut Search) };
        let mut pid = 0u32;
        // SAFETY: plain Win32 calls on a window handle EnumWindows just gave
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == search.pid {
            let mut buf = [0u16; 256];
            let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
            if n > 0 && buf[..n as usize] == search.title[..] {
                search.found.push(hwnd);
            }
        }
        1
    }

    let mut search = Search { pid: std::process::id(), title: title.encode_utf16().collect(), found: Vec::new() };
    // SAFETY: the callback only reads window data and writes into `search`
    unsafe { EnumWindows(visit, &mut search as *mut Search as isize) };
    let on: i32 = 1;
    for &hwnd in &search.found {
        // SAFETY: a window of this process; the value is a BOOL, as the attribute expects
        let hr = unsafe { DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const i32 as *const c_void, 4) };
        if hr != 0 {
            crate::log_error(&format!("could not keep the output window shown during Peek (0x{hr:08x})"));
        }
    }
    !search.found.is_empty()
}

#[cfg(not(windows))]
pub fn keep_shown(_title: &str) -> bool {
    true
}
