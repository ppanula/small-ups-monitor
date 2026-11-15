//! Shared, UI-independent UPS logic: HID transport, report decoding,
//! background polling and the tray-icon bitmap.

pub mod decode;
pub mod demo;
pub mod history;
pub mod icon;
pub mod poller;

#[cfg(feature = "win32")]
pub mod generic;
#[cfg(feature = "win32")]
pub mod hid_win32;
#[cfg(feature = "win32")]
pub mod hidmap;
#[cfg(feature = "win32")]
pub mod report;

pub use decode::{Date, Snapshot, UpsState};
pub use history::History;
pub use poller::{DeviceInfo, Event, Report, Transport};

/// Format seconds as "63 min" / "1 h 05 min".
pub fn fmt_runtime(secs: u32) -> String {
    let m = secs / 60;
    if m >= 60 {
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}
