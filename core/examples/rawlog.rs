//! Log raw UPS feature reports once per second, printing a line whenever any
//! value changes (plus a heartbeat every 30 s).
//!
//!   cargo run --release -p ups-core --features win32 --example rawlog -- [seconds]

use std::collections::BTreeMap;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ups_core::hid_win32::{APC_VID, ApcHid, ups_interfaces};

/// Known reports plus the unmapped vendor ones (docs/APC-UPS-GUIDE.md, section 5.6).
const IDS: &[u8] = &[
    0x06, 0x0C, 0x13, 0x14, 0x09, 0x31, 0x50, 0x36, 0x21, 0x15, 0x41, 0x42, 0x40, 0x18, 0x35, // known
    0x22, 0x23, 0x26, // battery collection duplicates
    0x51, 0x27, 0x34, 0x28, 0x60, 0x61, 0x62, 0x74, 0x75, 0x8C, 0x8D, 0x8E, 0x8F, 0x90, 0x91, 0x92,
    0x80, // vendor
];

fn now() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let s = (ms / 1000) % 86400;
    // UTC clock time; good enough to correlate with the app's local log (offset is constant).
    format!("{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

fn main() {
    let secs: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(600);
    let mut dev = loop {
        let found = ups_interfaces()
            .into_iter()
            .find(|i| i.vid == APC_VID)
            .ok_or("APC UPS not found".to_string());
        match found.and_then(|i| ApcHid::open_iface(&i)) {
            Ok(d) => break d,
            Err(e) => {
                println!("{} open failed: {e}", now());
                sleep(Duration::from_secs(2));
            }
        }
    };
    println!("{} opened {} SN {}", now(), dev.info().product, dev.info().serial);

    let start = Instant::now();
    let mut last: BTreeMap<u8, String> = BTreeMap::new();
    let mut last_beat = Instant::now();
    while start.elapsed().as_secs() < secs {
        let t = Instant::now();
        let mut changed = Vec::new();
        for &id in IDS {
            let v = match dev.feature(id) {
                Ok(r) => r[1..5].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "),
                Err(e) => format!("ERR {e}"),
            };
            if last.get(&id) != Some(&v) {
                changed.push(format!("{id:02X}=[{v}]"));
                last.insert(id, v);
            }
        }
        if !changed.is_empty() {
            println!("{} {}", now(), changed.join("  "));
        } else if last_beat.elapsed().as_secs() >= 30 {
            println!("{} (no change)", now());
            last_beat = Instant::now();
        }
        if !changed.is_empty() {
            last_beat = Instant::now();
        }
        if let Some(rest) = Duration::from_secs(1).checked_sub(t.elapsed()) {
            sleep(rest);
        }
    }
    println!("{} done", now());
}
