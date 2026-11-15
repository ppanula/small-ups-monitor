#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Small UPS Monitor: raw Win32 + GDI, hid.dll direct.
//!
//! Every connected HID UPS gets its own window and tray icon. APC units are
//! read with the APC reader; any other HID UPS with the generic reader.
//!
//! Command line:
//!   --tray   start hidden in the notification area (used by "Start with Windows")
//!   --demo   simulated UPS that cycles through outage states
//!   --report write a UPS report (for adding/checking UPS support), open it, exit

mod autostart;
mod eventlog;
mod gfx;
mod report;
mod settings;
mod tray;
mod ui;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ups_core::demo::DemoHid;
use ups_core::hid_win32::{UpsHid, ups_interfaces};
use ups_core::{DeviceInfo, Event, History, Snapshot, UpsState, fmt_runtime, poller};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{InvalidateRect, MONITOR_DEFAULTTONULL, MonitorFromPoint, ValidateRect};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::eventlog::EventLog;
use crate::settings::{DEFAULT_POS, Settings};

pub const APP_NAME: &str = "Small UPS Monitor";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO_URL: &str = "https://github.com/ppanula/small-ups-monitor";

pub const WM_APP_UPS: u32 = WM_APP + 1;
pub const WM_APP_TRAY: u32 = WM_APP + 2;
/// To the controller window: a UPS appeared. LPARAM = Box<String> (its id);
/// WPARAM = 1 if it was already there when the app started.
const WM_APP_DEVICE: u32 = WM_APP + 3;
/// To the controller window: a UPS has been gone from USB for REMOVE_AFTER. LPARAM = Box<String>.
const WM_APP_DEVICE_GONE: u32 = WM_APP + 4;
/// How long a UPS must be missing from USB before its window and tray icon are removed.
const REMOVE_AFTER: Duration = Duration::from_secs(10);
const TIMER_TICK: usize = 1;
const STALE_SECS: u64 = 10;
const HISTORY_LEN: usize = 300; // 10 min at 2 s
const SAMPLE_PERIOD: Duration = Duration::from_secs(2); // poller's fast cadence
const SCAN_PERIOD: Duration = Duration::from_secs(3);

pub const CLIENT_W: i32 = 380;
pub const CLIENT_H: i32 = 632;
/// Height of the "Please connect UPS" window.
pub const PLACEHOLDER_H: i32 = 150;
const STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
const CASCADE: i32 = 40;

const CMD_SHOW: usize = 1;
const CMD_TOPMOST: usize = 2;
const CMD_NOTIFY: usize = 3;
const CMD_AUTOSTART: usize = 4;
const CMD_OPEN_LOG: usize = 5;
const CMD_REPORT: usize = 6;
const CMD_REPORT_GITHUB: usize = 7;
const CMD_RELEASES: usize = 8;
/// Window (title-bar) menu item; system menu IDs must be below 0xF000 and multiples of 16.
const SC_ABOUT: usize = 0x0010;
const CMD_EXIT: usize = 9;

/// State of one UPS window.
pub struct App {
    pub hwnd: HWND,
    /// UPS id this window shows; empty while it is still searching.
    pub device_id: String,
    pub dpi: u32,
    pub fonts: gfx::Fonts,
    pub info: Option<DeviceInfo>,
    pub snap: Option<Snapshot>,
    pub last_data: Option<Instant>,
    /// Time of the newest chart slot (real sample or gap).
    last_sample: Option<Instant>,
    pub error: Option<String>,
    pub hist_v: History,
    pub hist_load: History,
    inbox: Arc<Mutex<Vec<Event>>>,
    tray: tray::Tray,
    prev_state: Option<UpsState>,
    pub outage_start: Option<Instant>,
    pub last_outage: Option<eventlog::Outage>,
    was_connected: bool,
    /// Tells this window's poller thread to finish.
    stop: Arc<AtomicBool>,
}

/// State shared by all windows.
struct Shared {
    settings: Settings,
    log: EventLog,
    demo: bool,
    start_hidden: bool,
    class: Vec<u16>,
    /// UPS id -> window.
    bound: HashMap<String, isize>,
    /// The "Please connect UPS" window, shown while no UPS is connected.
    searching: Option<isize>,
    /// Exit was chosen: the last window closing ends the app.
    exiting: bool,
}

thread_local! {
    static APPS: RefCell<HashMap<isize, App>> = RefCell::new(HashMap::new());
    static SHARED: RefCell<Option<Shared>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// Run `f` with one window's state; None if missing or already borrowed (re-entrancy).
fn with_app<R>(hwnd: HWND, f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APPS.with(|cell| cell.try_borrow_mut().ok()?.get_mut(&(hwnd as isize)).map(f))
}

fn with_shared<R>(f: impl FnOnce(&mut Shared) -> R) -> Option<R> {
    SHARED.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

fn all_windows() -> Vec<HWND> {
    APPS.with(|cell| cell.borrow().keys().map(|&h| h as HWND).collect())
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn fmt_duration(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min {:02} s", s / 60, s % 60),
        s => format!("{} h {:02} min", s / 3600, s / 60 % 60),
    }
}

impl App {
    pub fn connected(&self) -> bool {
        self.error.is_none() && self.last_data.is_some_and(|t| t.elapsed().as_secs() < STALE_SECS)
    }

    pub fn state(&self) -> UpsState {
        self.snap.as_ref().map_or(UpsState::Unknown, Snapshot::state)
    }

    pub fn model(&self) -> &str {
        self.info.as_ref().map_or("UPS", |i| i.model())
    }

    fn notify(&self, title: &str, text: &str, warning: bool) {
        if with_shared(|s| s.settings.notify).unwrap_or(true) {
            self.tray.balloon(&format!("{title} · {}", self.model()), text, warning);
        }
    }

    fn log(&self, event: &str, detail: &str) {
        let id = &self.device_id;
        with_shared(|s| s.log.write(event, detail, id));
    }

    fn drain(&mut self) {
        let events = std::mem::take(&mut *self.inbox.lock().unwrap());
        for ev in events {
            match ev {
                Event::Connected(info) => {
                    self.log(
                        "ups_connected",
                        &format!("{} SN {} ({})", info.product, info.serial, info.reader),
                    );
                    let title = if info.reader == "demo" {
                        "Small UPS Monitor (demo)".to_string()
                    } else {
                        format!("Small UPS Monitor · {}", info.model())
                    };
                    unsafe { SetWindowTextW(self.hwnd, wide(&title).as_ptr()) };
                    self.info = Some(info);
                    self.error = None;
                }
                Event::Data(s) => {
                    let now = Instant::now();
                    self.pad_history(now);
                    self.hist_v.push(s.input_v.map_or(f32::NAN, f32::from));
                    self.hist_load.push(s.load_pct.map_or(f32::NAN, f32::from));
                    self.last_sample = Some(now);
                    self.on_state(&s);
                    self.snap = Some(s);
                    self.last_data = Some(now);
                    self.error = None;
                }
                Event::Disconnected(e) => self.error = Some(e),
            }
        }
        self.refresh();
    }

    /// Notifications and event log on state transitions.
    fn on_state(&mut self, s: &Snapshot) {
        let now = s.state();
        let detail = format!(
            "{}% charge, {} runtime",
            s.charge.unwrap_or(0),
            fmt_runtime(s.runtime_s.unwrap_or(0))
        );
        match self.prev_state {
            Some(prev) if !prev.on_battery() && now.on_battery() => {
                self.outage_start = Some(Instant::now());
                self.last_outage = Some(eventlog::Outage {
                    start: eventlog::local_timestamp(),
                    secs: None,
                });
                let load = s.load_pct.map(|l| format!(", load {l}%")).unwrap_or_default();
                self.log("power_failure", &format!("{detail}{load}"));
                self.notify("Power failure", &format!("Running on battery. {detail}"), true);
            }
            Some(prev) if prev.on_battery() && !now.on_battery() => {
                let secs = self.outage_start.take().map(|t| t.elapsed().as_secs());
                if let (Some(o), Some(s)) = (self.last_outage.as_mut(), secs) {
                    o.secs = Some(s);
                }
                let took = secs
                    .map(fmt_duration)
                    .map(|d| format!(" after {d}"))
                    .unwrap_or_default();
                self.log("power_restored", &format!("on battery{took}, {detail}"));
                self.notify("Power restored", &format!("Back on mains power{took}."), false);
            }
            _ => {}
        }
        if now == UpsState::LowBattery && self.prev_state != Some(UpsState::LowBattery) {
            self.log("low_battery", &detail);
            self.notify("Battery low", &detail, true);
        }
        if now == UpsState::ShutdownImminent && self.prev_state != Some(UpsState::ShutdownImminent) {
            self.log("shutdown_imminent", &detail);
            self.notify(
                "UPS shutting down",
                "The UPS reports that it will shut down soon.",
                true,
            );
        }
        if s.need_replacement == Some(true) && self.snap.as_ref().and_then(|p| p.need_replacement) != Some(true) {
            self.log("replace_battery", "the UPS reports that its battery needs replacing");
            self.notify(
                "Replace battery",
                "The UPS reports that its battery needs replacing.",
                true,
            );
        }
        self.prev_state = Some(now);
    }

    /// Keep the charts' time axis honest: for every sample slot that passed
    /// without data (USB unplugged, UPS not answering, PC asleep), add a gap.
    fn pad_history(&mut self, now: Instant) {
        let Some(t) = self.last_sample else { return };
        let missed = (now.saturating_duration_since(t).as_secs_f32() / SAMPLE_PERIOD.as_secs_f32()) as usize;
        if missed < 2 {
            return; // normal poll jitter
        }
        let gaps = missed - 1;
        for _ in 0..gaps.min(HISTORY_LEN) {
            self.hist_v.push_gap();
            self.hist_load.push_gap();
        }
        self.last_sample = Some(t + SAMPLE_PERIOD * gaps as u32);
    }

    /// Update tray + window icon and repaint if visible.
    fn refresh(&mut self) {
        let connected = self.connected();
        if !connected {
            self.pad_history(Instant::now());
        }
        if self.was_connected && !connected {
            let why = self.error.clone().unwrap_or_else(|| "No data from UPS".into());
            self.log("ups_disconnected", &why);
            self.notify("UPS disconnected", &why, true);
        }
        self.was_connected = connected;

        let state = self.state();
        let charge = self.snap.as_ref().and_then(|s| s.charge);
        let tip = match (&self.snap, connected) {
            (Some(s), true) => format!(
                "{}: {} · {}% · {}{}",
                self.model(),
                state.label().to_lowercase(),
                s.charge.unwrap_or(0),
                fmt_runtime(s.runtime_s.unwrap_or(0)),
                s.load_pct.map(|l| format!(" · load {l}%")).unwrap_or_default()
            ),
            _ if self.device_id.is_empty() => "Small UPS Monitor: please connect a UPS".to_string(),
            _ => format!("{}: disconnected", self.model()),
        };
        self.tray.update(self.dpi, state, connected, charge, &tip);
        unsafe {
            if IsWindowVisible(self.hwnd) != 0 {
                InvalidateRect(self.hwnd, null(), 0);
            }
        }
    }

    fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        self.fonts = gfx::Fonts::new(dpi);
        self.tray.force_icon_refresh();
        self.refresh();
    }

    fn save_position(&self) {
        let mut r: RECT = unsafe { zeroed() };
        if unsafe { IsIconic(self.hwnd) } == 0 && unsafe { GetWindowRect(self.hwnd, &mut r) } != 0 {
            let key = if self.device_id.is_empty() {
                DEFAULT_POS.to_string()
            } else {
                self.device_id.clone()
            };
            with_shared(|s| {
                s.settings.positions.insert(key, (r.left, r.top));
                s.settings.save();
            });
        }
    }
}

/// Window size for a client area of CLIENT_W x `client_h` logical pixels.
fn client_to_window(dpi: u32, client_h: i32) -> (i32, i32) {
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: CLIENT_W * dpi as i32 / 96,
        bottom: client_h * dpi as i32 / 96,
    };
    unsafe { AdjustWindowRectExForDpi(&mut rc, STYLE, 0, 0, dpi) };
    (rc.right - rc.left, rc.bottom - rc.top)
}

unsafe fn style_title_bar(hwnd: HWND) {
    let dark: i32 = 1;
    let caption: u32 = gfx::BG;
    let corners: i32 = 2; // DWMWCP_ROUND
    unsafe {
        DwmSetWindowAttribute(hwnd, 20, (&dark as *const i32).cast(), 4); // DWMWA_USE_IMMERSIVE_DARK_MODE
        DwmSetWindowAttribute(hwnd, 35, (&caption as *const u32).cast(), 4); // DWMWA_CAPTION_COLOR
        DwmSetWindowAttribute(hwnd, 33, (&corners as *const i32).cast(), 4); // DWMWA_WINDOW_CORNER_PREFERENCE
    }
}

unsafe fn show_window(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, if IsIconic(hwnd) != 0 { SW_RESTORE } else { SW_SHOW });
        SetForegroundWindow(hwnd);
        InvalidateRect(hwnd, null(), 0);
    }
}

unsafe fn toggle_window(hwnd: HWND) {
    unsafe {
        let visible = IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0;
        if visible && GetForegroundWindow() == hwnd {
            ShowWindow(hwnd, SW_HIDE);
        } else {
            show_window(hwnd);
        }
    }
}

fn set_topmost(hwnd: HWND, on: bool) {
    let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe { SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
}

/// Saved position for `key`, if it is still on a connected monitor.
fn saved_position(key: &str) -> Option<(i32, i32)> {
    let (x, y) = with_shared(|s| s.settings.positions.get(key).copied()).flatten()?;
    let mon = unsafe { MonitorFromPoint(POINT { x: x + 40, y: y + 20 }, MONITOR_DEFAULTTONULL) };
    (!mon.is_null()).then_some((x, y))
}

/// Create a UPS window (hidden; the caller shows it).
fn create_window(device_id: &str) -> HWND {
    let class = with_shared(|s| s.class.clone()).unwrap_or_default();
    let key = if device_id.is_empty() { DEFAULT_POS } else { device_id };
    // New UPS without a saved place: cascade from the newest window.
    let pos = saved_position(key).or_else(|| {
        all_windows().last().and_then(|&h| {
            let mut r: RECT = unsafe { zeroed() };
            (unsafe { GetWindowRect(h, &mut r) } != 0).then_some((r.left + CASCADE, r.top + CASCADE))
        })
    });
    let (x, y) = pos.unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));
    unsafe {
        let hinst = GetModuleHandleW(null());
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide(APP_NAME).as_ptr(),
            STYLE,
            x,
            y,
            CLIENT_W,
            CLIENT_H,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        let dpi = GetDpiForWindow(hwnd);
        let (w, h) = client_to_window(dpi, if device_id.is_empty() { PLACEHOLDER_H } else { CLIENT_H });
        SetWindowPos(hwnd, null_mut(), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER);
        style_title_bar(hwnd);
        // "About" in the window's title-bar menu.
        let sys = GetSystemMenu(hwnd, 0);
        AppendMenuW(sys, MF_SEPARATOR, 0, null());
        AppendMenuW(sys, MF_STRING, SC_ABOUT, wide(&format!("About {APP_NAME}…")).as_ptr());
        if with_shared(|s| s.settings.topmost).unwrap_or(false) {
            set_topmost(hwnd, true);
        }
        let mut tray = tray::Tray::new(hwnd);
        tray.add(dpi);
        let app = App {
            hwnd,
            device_id: device_id.to_string(),
            dpi,
            fonts: gfx::Fonts::new(dpi),
            info: None,
            snap: None,
            last_data: None,
            last_sample: None,
            error: None,
            hist_v: History::new(HISTORY_LEN),
            hist_load: History::new(HISTORY_LEN),
            inbox: Arc::default(),
            tray,
            prev_state: None,
            outage_start: None,
            last_outage: None,
            was_connected: false,
            stop: Arc::new(AtomicBool::new(false)),
        };
        APPS.with(|cell| cell.borrow_mut().insert(hwnd as isize, app));
        SetTimer(hwnd, TIMER_TICK, 1000, None);
        hwnd
    }
}

/// Give UPS `id` a window (reusing the "Please connect UPS" window if it is
/// open) and start polling it. `at_startup`: the UPS was already connected when
/// the app started (then `--tray` decides visibility); otherwise it was just
/// plugged in and its window pops up.
fn bind_device(id: &str, at_startup: bool) {
    if with_shared(|s| s.bound.contains_key(id)).unwrap_or(true) {
        return;
    }
    let (demo, hidden, reuse) = with_shared(|s| (s.demo, s.start_hidden, s.searching.take())).unwrap();
    let hwnd = match reuse {
        Some(h) => h as HWND,
        None => create_window(id),
    };
    // Assign the UPS before any move/resize: a move to a monitor with other
    // scaling sends WM_DPICHANGED, which sizes the window by its kind.
    let last_outage = with_shared(|s| s.log.last_outage(id)).flatten();
    let (inbox, stop) = with_app(hwnd, |a| {
        a.device_id = id.to_string();
        a.last_outage = last_outage;
        (a.inbox.clone(), a.stop.clone())
    })
    .unwrap();
    if reuse.is_some() {
        // The "Please connect UPS" window becomes this UPS's window: move it to
        // the UPS's saved place first, then size it for that monitor's DPI.
        let flags = SWP_NOZORDER | SWP_NOACTIVATE;
        if let Some((x, y)) = saved_position(id) {
            unsafe { SetWindowPos(hwnd, null_mut(), x, y, 0, 0, flags | SWP_NOSIZE) };
        }
        let (w, hgt) = client_to_window(unsafe { GetDpiForWindow(hwnd) }, CLIENT_H);
        unsafe { SetWindowPos(hwnd, null_mut(), 0, 0, w, hgt, flags | SWP_NOMOVE) };
    }
    unsafe {
        if !at_startup {
            show_window(hwnd); // plugged in: pop up
        } else if !hidden {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
    with_shared(|s| s.bound.insert(id.to_string(), hwnd as isize));

    let target = hwnd as isize;
    let on_event = move |ev| {
        inbox.lock().unwrap().push(ev);
        unsafe { PostMessageW(target as HWND, WM_APP_UPS, 0, 0) };
    };
    if demo {
        poller::spawn::<DemoHid, _>(id.to_string(), stop, on_event);
    } else {
        poller::spawn::<UpsHid, _>(id.to_string(), stop, on_event);
    }
}

/// Show the "Please connect UPS" window (when no UPS window is left).
fn open_placeholder(visible: bool) {
    let hwnd = create_window("");
    with_shared(|s| s.searching = Some(hwnd as isize));
    with_app(hwnd, App::refresh);
    if visible {
        unsafe { ShowWindow(hwnd, SW_SHOW) };
    }
}

/// UPS `id` has been gone from USB for a while: close its window and tray icon.
fn unbind_device(id: &str) {
    let Some(h) = with_shared(|s| s.bound.remove(id)).flatten() else {
        return;
    };
    let hwnd = h as HWND;
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    with_app(hwnd, |a| a.log("ups_removed", "unplugged; window closed"));
    unsafe { DestroyWindow(hwnd) };
    if all_windows().is_empty() {
        open_placeholder(visible);
    }
}

/// Watch USB for UPSes: announce each one that appears, and each one that has
/// been missing for REMOVE_AFTER (a short glitch doesn't close its window).
fn spawn_device_watcher(controller: HWND) {
    let target = controller as isize;
    let post = move |msg: u32, at_startup: bool, id: String| {
        let boxed = Box::into_raw(Box::new(id)) as isize;
        unsafe { PostMessageW(target as HWND, msg, usize::from(at_startup), boxed) };
    };
    std::thread::Builder::new()
        .name("ups-watcher".into())
        .spawn(move || {
            let mut known: HashSet<String> = HashSet::new();
            let mut missing_since: HashMap<String, Instant> = HashMap::new();
            let mut first_scan = true;
            loop {
                let present: HashSet<String> = ups_interfaces().iter().map(|i| i.id()).collect();
                for id in &present {
                    missing_since.remove(id);
                    if known.insert(id.clone()) {
                        post(WM_APP_DEVICE, first_scan, id.clone());
                    }
                }
                let now = Instant::now();
                let gone: Vec<String> = known
                    .iter()
                    .filter(|id| !present.contains(*id))
                    .filter(|id| now - *missing_since.entry((*id).clone()).or_insert(now) >= REMOVE_AFTER)
                    .cloned()
                    .collect();
                for id in gone {
                    known.remove(&id);
                    missing_since.remove(&id);
                    post(WM_APP_DEVICE_GONE, false, id);
                }
                first_scan = false;
                std::thread::sleep(SCAN_PERIOD);
            }
        })
        .expect("spawn watcher");
}

/// About box: version, license, and an offer to open the project page.
unsafe fn show_about(hwnd: HWND) {
    let text = format!(
        "{APP_NAME} {VERSION}\n\nA small Windows monitor for USB HID UPSes.\nMIT License\n\n{REPO_URL}\n\nOpen the project page to check for updates?"
    );
    let answer = unsafe {
        MessageBoxW(
            hwnd,
            wide(&text).as_ptr(),
            wide(&format!("About {APP_NAME}")).as_ptr(),
            MB_YESNO | MB_ICONINFORMATION,
        )
    };
    if answer == IDYES {
        report::open(&format!("{REPO_URL}/releases"));
    }
}

/// Tray right-click menu. Choices arrive as WM_COMMAND.
unsafe fn tray_menu(hwnd: HWND) {
    let (topmost, notify) = with_shared(|s| (s.settings.topmost, s.settings.notify)).unwrap_or_default();
    let check = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
    unsafe {
        let menu = CreatePopupMenu();
        // Name and version on top; clicking it opens the Releases page (to check for updates).
        AppendMenuW(
            menu,
            MF_STRING,
            CMD_RELEASES,
            wide(&format!("{APP_NAME} {VERSION}")).as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, CMD_SHOW, wide("Show / hide").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(
            menu,
            MF_STRING | check(topmost),
            CMD_TOPMOST,
            wide("Always on top").as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING | check(notify),
            CMD_NOTIFY,
            wide("Notifications").as_ptr(),
        );
        AppendMenuW(
            menu,
            MF_STRING | check(autostart::is_enabled()),
            CMD_AUTOSTART,
            wide("Start with Windows").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, CMD_OPEN_LOG, wide("Open event log").as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_REPORT, wide("Create UPS report…").as_ptr());
        AppendMenuW(
            menu,
            MF_STRING,
            CMD_REPORT_GITHUB,
            wide("Report a UPS on GitHub…").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, CMD_EXIT, wide("Exit").as_ptr());
        SetMenuDefaultItem(menu, CMD_SHOW as u32, 0);
        let mut pt: POINT = zeroed();
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }
}

unsafe fn command(hwnd: HWND, id: usize) {
    unsafe {
        match id {
            CMD_SHOW => toggle_window(hwnd),
            CMD_TOPMOST => {
                let on = with_shared(|s| {
                    s.settings.topmost = !s.settings.topmost;
                    s.settings.save();
                    s.settings.topmost
                })
                .unwrap_or(false);
                for w in all_windows() {
                    set_topmost(w, on);
                }
            }
            CMD_NOTIFY => {
                with_shared(|s| {
                    s.settings.notify = !s.settings.notify;
                    s.settings.save();
                });
            }
            CMD_AUTOSTART => {
                autostart::set(!autostart::is_enabled());
            }
            CMD_OPEN_LOG => {
                let target = with_shared(|s| s.log.path().cloned()).flatten().map(|p| {
                    if p.exists() {
                        p
                    } else {
                        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or(p);
                        let _ = std::fs::create_dir_all(&dir);
                        dir
                    }
                });
                if let Some(t) = target {
                    let path = wide(&t.display().to_string());
                    ShellExecuteW(
                        hwnd,
                        wide("open").as_ptr(),
                        path.as_ptr(),
                        null(),
                        null(),
                        SW_SHOWNORMAL,
                    );
                }
            }
            CMD_REPORT => report::create_and_open(),
            CMD_REPORT_GITHUB => report::open(report::ISSUE_URL),
            CMD_RELEASES => report::open(&format!("{REPO_URL}/releases")),
            CMD_EXIT => {
                with_shared(|s| s.exiting = true);
                for w in all_windows() {
                    DestroyWindow(w);
                }
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                if with_app(hwnd, ui::paint).is_none() {
                    ValidateRect(hwnd, null());
                }
                0
            }
            WM_APP_UPS => {
                with_app(hwnd, App::drain);
                0
            }
            WM_TIMER => {
                with_app(hwnd, App::refresh);
                0
            }
            WM_APP_TRAY => {
                match (lp as u32) & 0xFFFF {
                    WM_LBUTTONUP => toggle_window(hwnd),
                    WM_RBUTTONUP => tray_menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_COMMAND => {
                command(hwnd, wp & 0xFFFF);
                0
            }
            WM_SYSCOMMAND if wp & 0xFFF0 == SC_ABOUT => {
                show_about(hwnd);
                0
            }
            WM_CLOSE => {
                ShowWindow(hwnd, SW_HIDE); // close = hide to tray; exit from the tray menu
                0
            }
            WM_DPICHANGED => {
                let dpi = ((wp >> 16) & 0xFFFF) as u32;
                let r = &*(lp as *const RECT);
                let placeholder = with_app(hwnd, |a| a.device_id.is_empty()).unwrap_or(false);
                let (w, h) = client_to_window(dpi, if placeholder { PLACEHOLDER_H } else { CLIENT_H });
                SetWindowPos(hwnd, null_mut(), r.left, r.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                with_app(hwnd, |a| a.set_dpi(dpi));
                0
            }
            WM_ENDSESSION => {
                if wp != 0 {
                    with_app(hwnd, |a| {
                        a.save_position();
                        a.tray.remove();
                    });
                }
                0
            }
            WM_DESTROY => {
                KillTimer(hwnd, TIMER_TICK);
                with_app(hwnd, |a| {
                    a.save_position();
                    a.tray.remove();
                    a.stop.store(true, Ordering::Relaxed);
                });
                let remaining = APPS.with(|cell| {
                    let mut apps = cell.borrow_mut();
                    apps.remove(&(hwnd as isize));
                    apps.len()
                });
                with_shared(|s| {
                    if s.searching == Some(hwnd as isize) {
                        s.searching = None;
                    }
                });
                if remaining == 0 && with_shared(|s| s.exiting).unwrap_or(true) {
                    PostQuitMessage(0);
                }
                0
            }
            m if m != 0 && m == TASKBAR_CREATED.get() => {
                with_app(hwnd, |a| a.tray.re_add());
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Hidden message-only window that receives "UPS appeared" from the watcher.
unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_APP_DEVICE {
            let id = *Box::from_raw(lp as *mut String);
            bind_device(&id, wp != 0);
            return 0;
        }
        if msg == WM_APP_DEVICE_GONE {
            let id = *Box::from_raw(lp as *mut String);
            unbind_device(&id);
            return 0;
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--report") {
        // Write a UPS report, open it, and exit (works while the app is running too).
        settings::migrate_legacy_dir();
        if let Some(p) = report::write_report() {
            report::open(&p.display().to_string());
        }
        return;
    }
    let demo = args.iter().any(|a| a == "--demo");
    let start_hidden = args.iter().any(|a| a == "--tray");
    let class = wide(if demo { "SmallUpsMonitorDemo" } else { "SmallUpsMonitor" });

    unsafe {
        // Single instance (separately for demo): bring a running window forward instead.
        let mutex_name = wide(if demo {
            "Local\\SmallUpsMonitor.demo"
        } else {
            "Local\\SmallUpsMonitor"
        });
        let _mutex = CreateMutexW(null(), 1, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(class.as_ptr(), null());
            if !existing.is_null() {
                show_window(existing);
            }
            return;
        }

        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        settings::migrate_legacy_dir();
        TASKBAR_CREATED.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        if !demo {
            autostart::refresh_path();
        }

        let hinst = GetModuleHandleW(null());
        // MAKEINTRESOURCE(1): the icon embedded by build.rs.
        let app_icon = LoadIconW(hinst, std::ptr::without_provenance(1));
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hIcon: app_icon,
            hIconSm: app_icon,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassExW(&wc);
        let ctl_class = wide("SmallUpsMonitorController");
        let ctl = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(controller_proc),
            hInstance: hinst,
            lpszClassName: ctl_class.as_ptr(),
            ..zeroed()
        };
        RegisterClassExW(&ctl);
        let controller = CreateWindowExW(
            0,
            ctl_class.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            hinst,
            null(),
        );

        SHARED.with(|cell| {
            *cell.borrow_mut() = Some(Shared {
                settings: Settings::load(),
                log: EventLog::new(demo),
                demo,
                start_hidden,
                class: class.clone(),
                bound: HashMap::new(),
                searching: None,
                exiting: false,
            })
        });

        // "Please connect UPS" until the first UPS is found; it becomes that UPS's window.
        open_placeholder(!start_hidden);

        if demo {
            bind_device("DEMO", true);
        } else {
            spawn_device_watcher(controller);
        }

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
