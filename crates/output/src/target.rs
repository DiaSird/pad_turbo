//! Background key delivery to the windows of a chosen process.
//!
//! The keyboard output normally injects keys with `SendInput`, so they reach
//! whichever window has focus. When the user picks a running process from
//! [`list_processes`] instead, [`WindowTarget`] posts `WM_KEYDOWN` /
//! `WM_KEYUP` straight to that process's windows, and the target keeps
//! receiving input while it is in the background.
//!
//! A target is an executable name plus, optionally, one process id. With a
//! process id only that process is driven; if it exits (or no id was given,
//! e.g. after PadTurbo restarts), every running process with the same
//! executable name is driven instead, so a restarted target is picked up
//! again automatically.
//!
//! Posted messages bypass the system input queue: programs that read the
//! keyboard through DirectInput, raw input or `GetAsyncKeyState` do not see
//! them. Programs driven by window messages (most emulators, browsers, tools
//! and many 2D games) do. Windows also refuses messages from a normal process
//! to an elevated one, so an elevated target needs PadTurbo to run elevated.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC, MapVirtualKeyW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_KEYDOWN, WM_KEYUP,
};

/// How often the target's window list is rebuilt, so that windows opened or
/// closed by the target (or a restarted target) are picked up.
const WINDOW_REFRESH: Duration = Duration::from_secs(1);

/// A running process offered in the GUI's target list.
pub struct ProcessInfo {
    /// Executable file name, e.g. `game.exe`.
    pub exe: String,
    /// Process id.
    pub pid: u32,
    /// Title of its main window; `None` for processes without a visible,
    /// titled top-level window (they cannot receive keys).
    pub title: Option<String>,
}

/// Lists every running process except PadTurbo itself.
///
/// Processes with a window come first, then the rest; each group is sorted
/// by executable name and process id.
pub fn list_processes() -> Vec<ProcessInfo> {
    // SAFETY: plain query without arguments.
    let own = unsafe { GetCurrentProcessId() };
    let mut titles: HashMap<u32, String> = HashMap::new();
    for hwnd in top_windows() {
        // SAFETY: `hwnd` came from EnumWindows; a window that has closed since
        // only makes these calls fail.
        let main = unsafe { IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() };
        if main {
            let title = window_title(hwnd);
            if !title.is_empty() {
                titles.entry(window_pid(hwnd)).or_insert(title);
            }
        }
    }
    let mut list: Vec<ProcessInfo> = processes()
        .into_iter()
        .filter(|&(pid, _)| pid != own && pid != 0)
        .map(|(pid, exe)| ProcessInfo {
            exe,
            pid,
            title: titles.remove(&pid),
        })
        .collect();
    list.sort_by_cached_key(|p| (p.title.is_none(), p.exe.to_lowercase(), p.pid));
    list
}

/// The windows of one process (or one executable) that keys are posted to.
pub struct WindowTarget {
    /// Executable file name the user chose.
    exe: String,
    /// Process the user chose, if a specific one was picked.
    pid: Option<u32>,
    /// Processes currently driven, from the last scan.
    pids: Vec<u32>,
    /// Visible top-level windows of [`WindowTarget::pids`], plus all their
    /// child windows (many programs handle keys in a child window).
    windows: Vec<HWND>,
    /// When [`WindowTarget::windows`] is rebuilt next.
    next_scan: Instant,
}

impl WindowTarget {
    /// Creates a target for process `pid` of `exe`, or for every process of
    /// `exe` (matched case-insensitively) when `pid` is `None`.
    pub fn new(exe: &str, pid: Option<u32>) -> Self {
        Self {
            exe: exe.to_string(),
            pid,
            pids: Vec::new(),
            windows: Vec::new(),
            next_scan: Instant::now(),
        }
    }

    /// The executable name and process id this target was created for.
    pub fn spec(&self) -> (&str, Option<u32>) {
        (&self.exe, self.pid)
    }

    /// Processes and number of windows keys are currently posted to; empty
    /// and 0 when the executable is not running.
    pub fn status(&mut self) -> (&[u32], usize) {
        self.refresh();
        (&self.pids, self.windows.len())
    }

    /// Posts a key press (`down`) or release of virtual-key code `vk` to every
    /// window of the target.
    ///
    /// `lParam` carries the scan code and, for keys on the extended page,
    /// the extended flag, as a real key event would. Failures (a window that
    /// closed since the last scan) are ignored.
    pub fn post_key(&mut self, vk: u16, down: bool, extended: bool) {
        self.refresh();
        // SAFETY: plain table lookup.
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) };
        let mut lparam = 1 | (scan & 0xFF) << 16;
        if extended {
            lparam |= 1 << 24;
        }
        // Key up: "previous state down" and "transition" bits.
        let msg = if down {
            WM_KEYDOWN
        } else {
            lparam |= 0b11 << 30;
            WM_KEYUP
        };
        for &hwnd in &self.windows {
            // SAFETY: PostMessageW accepts any handle and fails on stale ones.
            unsafe { PostMessageW(hwnd, msg, vk as usize, lparam as LPARAM) };
        }
    }

    /// Rebuilds the window list if [`WINDOW_REFRESH`] has passed.
    fn refresh(&mut self) {
        let now = Instant::now();
        if now < self.next_scan {
            return;
        }
        self.next_scan = now + WINDOW_REFRESH;
        let same_exe: Vec<u32> = processes()
            .into_iter()
            .filter(|(_, exe)| exe.eq_ignore_ascii_case(&self.exe))
            .map(|(pid, _)| pid)
            .collect();
        // The chosen process if it still runs, otherwise any with that name.
        self.pids = match self.pid {
            Some(pid) if same_exe.contains(&pid) => vec![pid],
            _ => same_exe,
        };
        self.windows.clear();
        for hwnd in top_windows() {
            // SAFETY: `hwnd` came from EnumWindows.
            if unsafe { IsWindowVisible(hwnd) } == 0 {
                continue;
            }
            if self.pids.contains(&window_pid(hwnd)) {
                self.windows.push(hwnd);
                self.windows.extend(child_windows(hwnd));
            }
        }
    }
}

/// `EnumWindows` / `EnumChildWindows` callback collecting handles into the
/// `Vec<HWND>` passed through `lparam`.
unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: `lparam` is the `&mut Vec<HWND>` passed by the callers below,
    // alive for the duration of the enumeration.
    let list = unsafe { &mut *(lparam as *mut Vec<HWND>) };
    list.push(hwnd);
    1
}

/// All top-level windows on the desktop.
fn top_windows() -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    // SAFETY: `collect` only touches `list`, which outlives the call.
    unsafe { EnumWindows(Some(collect), &raw mut list as LPARAM) };
    list
}

/// All descendants of `parent`.
fn child_windows(parent: HWND) -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    // SAFETY: as in `top_windows`.
    unsafe { EnumChildWindows(parent, Some(collect), &raw mut list as LPARAM) };
    list
}

/// Process id owning `hwnd` (0 if the window is gone).
fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: `pid` is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    pid
}

/// Title bar text of `hwnd`.
fn window_title(hwnd: HWND) -> String {
    // SAFETY: the buffer is sized from the reported length plus the NUL.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0_u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

/// `(process id, executable file name)` of every running process.
///
/// Uses a ToolHelp snapshot, which reports names even for elevated or
/// protected processes that cannot be opened. Returns an empty list if the
/// snapshot fails.
fn processes() -> Vec<(u32, String)> {
    let mut list = Vec::new();
    // SAFETY: the snapshot handle is checked and closed before returning;
    // `entry.dwSize` is set as Process32FirstW requires, and `szExeFile` is a
    // NUL-terminated buffer filled by the call.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return list;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &raw mut entry);
        while ok != 0 {
            let name = &entry.szExeFile;
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            list.push((entry.th32ProcessID, String::from_utf16_lossy(&name[..len])));
            ok = Process32NextW(snap, &raw mut entry);
        }
        CloseHandle(snap);
    }
    list
}
