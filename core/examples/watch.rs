//! Print each UPS's decoded values once per second whenever they change,
//! using the same readers as Small UPS Monitor (APC reader or generic HID).
//! Survives unplug / replug.
//!
//!   cargo run --release -p ups-core --features win32 --example watch -- [seconds] [id-substring]

use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ups_core::hid_win32::{UpsHid, ups_interfaces};
use ups_core::{Snapshot, Transport};

fn now() -> String {
    let s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() % 86400;
    format!("{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

fn line(s: &Snapshot) -> String {
    format!(
        "{:<13} {:<14} charge={:?} runtime={:?} load={:?} in={:?} batt={:?} over={:?} replace={:?}",
        s.state().label(),
        s.status_tokens(),
        s.charge,
        s.runtime_s,
        s.load_pct,
        s.input_v,
        s.battery_v,
        s.overload,
        s.need_replacement
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let secs: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(300);
    let filter = args.next().unwrap_or_default();
    let start = Instant::now();
    let mut dev: Option<UpsHid> = None;
    let mut snap = Snapshot::default();
    let mut last = String::new();
    while start.elapsed().as_secs() < secs {
        let t = Instant::now();
        if dev.is_none() {
            let found = ups_interfaces()
                .into_iter()
                .find(|i| i.id().contains(&filter) || i.product.contains(&filter));
            let msg = match found.map(|i| UpsHid::open_iface(&i)) {
                Some(Ok(d)) => {
                    let m = format!("connected {} ({})", d.info().product, d.info().reader);
                    dev = Some(d);
                    snap = Snapshot::default();
                    m
                }
                Some(Err(e)) => format!("open failed: {e}"),
                None => "not present".to_string(),
            };
            if msg != last {
                println!("{} {msg}", now());
                last = msg;
            }
        }
        if let Some(d) = dev.as_mut() {
            match d.read(&mut snap, true, false) {
                Ok(()) => {
                    let l = line(&snap);
                    if l != last {
                        println!("{} {l}", now());
                        last = l;
                    }
                }
                Err(e) => {
                    println!("{} read failed: {e}", now());
                    last.clear();
                    dev = None;
                }
            }
        }
        sleep(Duration::from_secs(1).saturating_sub(t.elapsed()));
    }
    println!("{} done", now());
}
