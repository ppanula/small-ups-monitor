//! Notification-area icon (Shell_NotifyIconW) with a runtime-drawn bitmap.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use ups_core::{UpsState, icon};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, GetDC,
    ReleaseDC,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICON_BIG, ICON_SMALL, ICONINFO, SM_CXICON, SM_CXSMICON,
    SendMessageW, WM_SETICON,
};

use crate::WM_APP_TRAY;

type Key = (UpsState, bool, u8);

/// `GetSystemMetrics` for a specific DPI, falling back to the process-wide
/// `GetSystemMetrics` on Windows 7 / Server 2008 R2 (where the exe is
/// system-DPI aware, so the returned size is already scaled).
fn system_metrics(index: i32, dpi: u32) -> i32 {
    if let Some(proc) = crate::user32_proc(b"GetSystemMetricsForDpi") {
        let get = unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn(i32, u32) -> i32>(
                proc,
            )
        };
        return unsafe { get(index, dpi) };
    }
    unsafe { GetSystemMetrics(index) }
}

pub struct Tray {
    hwnd: HWND,
    small: HICON,
    big: HICON,
    key: Option<Key>,
    tip: String,
    added: bool,
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    let max = dst.len() - 1;
    for (d, c) in dst.iter_mut().take(max).zip(s.encode_utf16()) {
        *d = c;
        n += 1;
    }
    dst[n] = 0;
}

/// Build an HICON from straight-alpha RGBA pixels.
pub fn make_icon(size: u32, state: UpsState, connected: bool, charge: Option<u8>) -> HICON {
    let rgba = icon::tray_rgba(size, state, connected, charge);
    unsafe {
        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits = null_mut();
        let dc = GetDC(null_mut());
        let color = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        ReleaseDC(null_mut(), dc);
        if color.is_null() || bits.is_null() {
            return null_mut();
        }
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, rgba.len());
        for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(rgba.as_chunks::<4>().0) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]); // RGBA -> BGRA
        }
        let mask_bits = vec![0u8; size.div_ceil(16) as usize * 2 * size as usize];
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, mask_bits.as_ptr().cast());
        let ii = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let h = CreateIconIndirect(&ii);
        DeleteObject(color);
        DeleteObject(mask);
        h
    }
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            small: null_mut(),
            big: null_mut(),
            key: None,
            tip: crate::APP_NAME.into(),
            added: false,
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut d: NOTIFYICONDATAW = unsafe { zeroed() };
        d.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        d.hWnd = self.hwnd;
        d.uID = 1;
        d
    }

    fn set_icons(&mut self, dpi: u32, key: Key) {
        let (state, connected, charge) = key;
        let sm = system_metrics(SM_CXSMICON, dpi).max(16) as u32;
        let lg = system_metrics(SM_CXICON, dpi).max(32) as u32;
        let small = make_icon(sm, state, connected, Some(charge));
        let big = make_icon(lg, state, connected, Some(charge));
        unsafe {
            SendMessageW(self.hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
            SendMessageW(self.hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
            if !self.small.is_null() {
                DestroyIcon(self.small);
            }
            if !self.big.is_null() {
                DestroyIcon(self.big);
            }
        }
        self.small = small;
        self.big = big;
        self.key = Some(key);
    }

    pub fn add(&mut self, dpi: u32) {
        self.set_icons(dpi, (UpsState::Unknown, false, 100));
        self.re_add();
    }

    /// (Re-)register the icon, e.g. after Explorer restarts.
    pub fn re_add(&mut self) {
        let mut d = self.data();
        d.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE | NIF_SHOWTIP;
        d.uCallbackMessage = WM_APP_TRAY;
        d.hIcon = self.small;
        copy_wide(&mut d.szTip, &self.tip);
        self.added = unsafe { Shell_NotifyIconW(NIM_ADD, &d) } != 0;
    }

    pub fn force_icon_refresh(&mut self) {
        self.key = None;
    }

    pub fn update(&mut self, dpi: u32, state: UpsState, connected: bool, charge: Option<u8>, tip: &str) {
        // Charge is bucketed to 5 % so the icon is not rebuilt on every 1 % change.
        let key = (state, connected, charge.unwrap_or(100) / 5 * 5);
        let icon_changed = self.key != Some(key);
        if icon_changed {
            self.set_icons(dpi, key);
        }
        if !icon_changed && tip == self.tip {
            return;
        }
        self.tip = tip.to_string();
        let mut d = self.data();
        d.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        d.hIcon = self.small;
        copy_wide(&mut d.szTip, tip);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }

    pub fn balloon(&self, title: &str, text: &str, warning: bool) {
        let mut d = self.data();
        d.uFlags = NIF_INFO;
        copy_wide(&mut d.szInfoTitle, title);
        copy_wide(&mut d.szInfo, text);
        d.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
    }

    pub fn remove(&mut self) {
        if self.added {
            let d = self.data();
            unsafe { Shell_NotifyIconW(NIM_DELETE, &d) };
            self.added = false;
        }
    }
}
