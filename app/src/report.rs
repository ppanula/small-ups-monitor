//! "Create UPS report…" / `--report`: write the UPS report to a text file and
//! open it, so users can paste it into a GitHub issue.

use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::eventlog::local_timestamp;
use crate::settings::data_dir;
use crate::wide;

/// New-issue page with the UPS report form.
pub const ISSUE_URL: &str = "https://github.com/ppanula/small-ups-monitor/issues/new?template=ups-report.yml";

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

fn reg_string(name: &str) -> Option<String> {
    let mut buf = [0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(CURRENT_VERSION).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (rc == 0).then(|| {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    })
}

fn reg_dword(name: &str) -> Option<u32> {
    let mut v = 0u32;
    let mut size = 4u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(CURRENT_VERSION).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut v as *mut u32).cast(),
            &mut size,
        )
    };
    (rc == 0).then_some(v)
}

/// e.g. "Windows 11 Professional 25H2 (build 26300.1234)".
fn windows_version() -> String {
    let build: u32 = reg_string("CurrentBuildNumber")
        .and_then(|b| b.parse().ok())
        .unwrap_or(0);
    let name = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    let edition = reg_string("EditionID").unwrap_or_default();
    let display = reg_string("DisplayVersion").unwrap_or_default();
    let ubr = reg_dword("UBR").map(|u| format!(".{u}")).unwrap_or_default();
    format!("{name} {edition} {display} (build {build}{ubr})").replace("  ", " ")
}

/// Write a report into %APPDATA%\Small UPS Monitor\reports and return its path.
pub fn write_report() -> Option<PathBuf> {
    let stamp = local_timestamp();
    let text = ups_core::report::generate(env!("CARGO_PKG_VERSION"), &windows_version(), &stamp);
    let dir = data_dir()?.join("reports");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("ups-report-{}.txt", stamp.replace([' ', ':'], "-")));
    std::fs::write(&path, text).ok()?;
    Some(path)
}

/// Open a file or URL with its default program.
pub fn open(target: &str) {
    unsafe {
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(target).as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Create a report and open it (runs on a worker thread; reading takes a few seconds).
pub fn create_and_open() {
    std::thread::spawn(|| {
        if let Some(p) = write_report() {
            open(&p.display().to_string());
        }
    });
}
