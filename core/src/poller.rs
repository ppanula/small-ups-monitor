//! Background thread that owns one UPS, polls it and reports events.
//! HID reads can block, so they never run on the UI thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::Snapshot;

/// A raw report: `[report_id, data...]`, zero padded.
pub type Report = [u8; 8];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub manufacturer: String,
    /// HID product string, e.g. "Back-UPS BR1500GI FW:878.L3 .I USB FW:L3".
    pub product: String,
    pub serial: String,
    /// Stable identity used for settings and the event log: "VID:PID:serial".
    pub id: String,
    /// Which reader decodes this device, e.g. "APC reader" or "generic HID".
    pub reader: &'static str,
    pub quirks: Quirks,
}

/// Known model limitations, so the UI doesn't present meaningless values as data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quirks {
    /// The UPS reports a load, but it never reflects what is connected (always 0 %).
    pub load_not_measured: bool,
    /// The runtime is derived from the charge only, not from the load.
    pub runtime_from_charge: bool,
}

impl DeviceInfo {
    /// Model name without the firmware suffix.
    pub fn model(&self) -> &str {
        self.product.split(" FW:").next().unwrap_or(&self.product).trim()
    }

    /// Firmware part of the product string, e.g. "878.L3 .I  ·  USB L3".
    pub fn firmware(&self) -> Option<String> {
        let i = self.product.find(" FW:")?;
        Some(self.product[i + 4..].trim().replace(" USB FW:", "  ·  USB "))
    }

    /// Serial number worth showing (some UPSes report all zeros).
    pub fn real_serial(&self) -> Option<&str> {
        let s = self.serial.trim();
        (!s.is_empty() && s.chars().any(|c| c != '0')).then_some(s)
    }
}

/// A way to read one UPS.
pub trait Transport: Sized {
    /// Open the UPS with this `DeviceInfo::id` ("" = the first one found).
    fn open(id: &str) -> Result<Self, String>;
    fn info(&self) -> &DeviceInfo;
    /// Update `snap`. `slow` / `statics` ask for the slower-changing values too.
    /// Errors only when nothing at all could be read (device gone).
    fn read(&mut self, snap: &mut Snapshot, slow: bool, statics: bool) -> Result<(), String>;
}

#[derive(Debug)]
pub enum Event {
    Connected(DeviceInfo),
    Data(Snapshot),
    Disconnected(String),
}

const FAST_PERIOD: Duration = Duration::from_secs(2);
const SLOW_PERIOD: Duration = Duration::from_secs(30);
const STATIC_PERIOD: Duration = Duration::from_secs(600);
const RETRY: Duration = Duration::from_secs(3);

/// Sleep up to `d`; returns true as soon as `stop` is set.
fn nap(d: Duration, stop: &AtomicBool) -> bool {
    let end = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        thread::sleep(left.min(Duration::from_millis(200)));
    }
    true
}

/// Spawn a poller for the UPS `id`. It keeps retrying while the UPS is away,
/// until `stop` is set. `on_event` runs on the poller thread.
pub fn spawn<T, F>(id: String, stop: Arc<AtomicBool>, mut on_event: F) -> thread::JoinHandle<()>
where
    T: Transport + 'static,
    F: FnMut(Event) + Send + 'static,
{
    thread::Builder::new()
        .name(format!("ups-poller {id}"))
        .spawn(move || {
            let mut last_err = String::new();
            while !stop.load(Ordering::Relaxed) {
                let mut dev = match T::open(&id) {
                    Ok(d) => d,
                    Err(e) => {
                        if e != last_err {
                            on_event(Event::Disconnected(e.clone()));
                            last_err = e;
                        }
                        nap(RETRY, &stop);
                        continue;
                    }
                };
                last_err.clear();
                on_event(Event::Connected(dev.info().clone()));

                let mut snap = Snapshot::default();
                let mut last_slow: Option<Instant> = None;
                let mut last_static: Option<Instant> = None;
                loop {
                    let start = Instant::now();
                    let slow = last_slow.is_none_or(|t| start - t >= SLOW_PERIOD);
                    let statics = last_static.is_none_or(|t| start - t >= STATIC_PERIOD);
                    if let Err(e) = dev.read(&mut snap, slow, statics) {
                        on_event(Event::Disconnected(format!("UPS stopped responding: {e}")));
                        last_err = String::from("(lost)");
                        break;
                    }
                    if slow {
                        last_slow = Some(start);
                    }
                    if statics {
                        last_static = Some(start);
                    }
                    snap.timestamp = Some(SystemTime::now());
                    on_event(Event::Data(snap.clone()));
                    if nap(FAST_PERIOD.saturating_sub(start.elapsed()), &stop) {
                        break;
                    }
                }
                drop(dev);
                nap(Duration::from_secs(1), &stop);
            }
        })
        .expect("spawn poller thread")
}
