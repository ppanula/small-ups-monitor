//! "Start with Windows": a value under HKCU\...\CurrentVersion\Run.

use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};

use crate::wide;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "Small UPS Monitor";
/// Value name used by pre-1.0 builds.
const LEGACY_NAME: &str = "UPS Monitor";

struct Key(HKEY);

impl Key {
    fn open() -> Option<Self> {
        let mut h: HKEY = null_mut();
        let rc = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(RUN).as_ptr(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                &mut h,
            )
        };
        (rc == 0).then_some(Key(h))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

/// The command line registered for autostart: the current exe, starting in the tray.
pub fn command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!("\"{exe}\" --tray")
}

fn value(name: &str) -> Option<String> {
    let key = Key::open()?;
    let name = wide(name);
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let mut kind = 0u32;
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            null(),
            &mut kind,
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != 0 || kind != REG_SZ {
        return None;
    }
    let n = (size as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..n]);
    Some(s.trim_end_matches('\0').to_string())
}

fn current() -> Option<String> {
    value(NAME)
}

pub fn is_enabled() -> bool {
    current().is_some()
}

pub fn set(enabled: bool) -> bool {
    let Some(key) = Key::open() else { return false };
    let name = wide(NAME);
    let rc = if enabled {
        let data = wide(&command());
        unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteValueW(key.0, name.as_ptr()) }
    };
    rc == 0
}

/// Keep autostart pointing at this exe: take over a pre-1.0 entry, and
/// update the path if the exe has moved.
pub fn refresh_path() {
    if value(LEGACY_NAME).is_some() {
        if let Some(key) = Key::open() {
            unsafe { RegDeleteValueW(key.0, wide(LEGACY_NAME).as_ptr()) };
        }
        set(true);
        return;
    }
    if let Some(v) = current()
        && v != command()
    {
        set(true);
    }
}
