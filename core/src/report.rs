//! "UPS report": everything needed to add or check support for a UPS, in a
//! form users can paste into a GitHub issue.
//!
//! For each connected HID UPS it lists the identity (serial number hidden),
//! what Small UPS Monitor decodes, and the device's full report layout with
//! raw values from two samples a few seconds apart, so values that never
//! change stand out. No device paths or other machine details are included.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::thread::sleep;
use std::time::Duration;

use crate::generic::GenericHid;
use crate::hid_win32::{APC_VID, Handle, UpsHid, UpsInterface, open_path, ups_interfaces};
use crate::hidmap::{Kind, Layout};
use crate::{Snapshot, Transport, fmt_runtime};

/// Time between the two samples.
const SAMPLE_GAP: Duration = Duration::from_secs(3);

type Samples = BTreeMap<(Kind, u8), Result<Vec<u8>, u32>>;

struct Device {
    iface: UpsInterface,
    handle: Handle,
    layout: Layout,
    first: Samples,
}

fn sample(d: &Device) -> Samples {
    d.layout
        .reports()
        .into_iter()
        .map(|(k, id)| ((k, id), d.layout.fetch(&d.handle, k, id)))
        .collect()
}

fn serial_note(serial: &str) -> String {
    let s = serial.trim();
    if s.is_empty() {
        "none".into()
    } else if s.chars().all(|c| c == '0') {
        "all zeros".into()
    } else {
        format!("hidden ({} characters)", s.chars().count())
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "—".to_string(), |v| v.to_string())
}

/// The values the UI shows, as (label, value) rows.
fn rows(s: &Snapshot) -> Vec<(&'static str, String)> {
    vec![
        ("Status", format!("{} ({})", s.state().label(), s.status_tokens())),
        ("Charge %", opt(s.charge)),
        (
            "Runtime",
            opt(s.runtime_s.map(|r| format!("{r} s ({})", fmt_runtime(r)))),
        ),
        ("Load %", opt(s.load_pct)),
        ("Input voltage", opt(s.input_v)),
        ("Input nominal", opt(s.input_v_nominal)),
        (
            "Transfer low / high",
            format!("{} / {}", opt(s.transfer_low), opt(s.transfer_high)),
        ),
        ("Battery voltage", opt(s.battery_v.map(|v| format!("{v:.2}")))),
        ("Battery nominal", opt(s.battery_v_nominal.map(|v| format!("{v:.1}")))),
        ("Rated power W", opt(s.rated_w)),
        ("Rated power VA", opt(s.rated_va)),
        ("Beeper", opt(s.beeper)),
        ("Sensitivity", opt(s.sensitivity)),
        ("Self-test", opt(s.test_result)),
        ("Overload", opt(s.overload)),
        ("Replace battery", opt(s.need_replacement)),
        ("UPS made", opt(s.mfr_date)),
        ("Battery date", opt(s.battery_mfr_date)),
    ]
}

fn decode_with(dev: Result<UpsHid, String>) -> Result<(String, Snapshot), String> {
    let mut dev = dev?;
    let mut s = Snapshot::default();
    dev.read(&mut s, true, true)?;
    Ok((dev.info().reader.to_string(), s))
}

fn generic_only(iface: &UpsInterface) -> Result<(String, Snapshot), String> {
    let mut g = GenericHid::open_iface(iface)?;
    let mut s = Snapshot::default();
    g.read(&mut s, true, true)?;
    Ok(("generic HID".to_string(), s))
}

fn write_device(out: &mut String, n: usize, d: &Device, second: &Samples) {
    let i = &d.iface;
    let _ = writeln!(out, "## UPS {n}: {} {}\n", i.manufacturer, i.product);
    let _ = writeln!(out, "| | |\n|---|---|");
    let _ = writeln!(out, "| USB ID | `{:04X}:{:04X}` |", i.vid, i.pid);
    let _ = writeln!(out, "| Manufacturer | {} |", i.manufacturer);
    let _ = writeln!(out, "| Product | {} |", i.product);
    let _ = writeln!(out, "| Serial number | {} |", serial_note(&i.serial));
    let info = i.info(if i.vid == APC_VID { "APC reader" } else { "generic HID" });
    let q = info.quirks;
    let quirks = [
        (q.load_not_measured, "load not measured"),
        (q.runtime_from_charge, "runtime from charge only"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, s)| *s)
    .collect::<Vec<_>>();
    let _ = writeln!(out, "| Reader used | {} |", info.reader);
    let _ = writeln!(
        out,
        "| Known quirks | {} |\n",
        if quirks.is_empty() {
            "none".to_string()
        } else {
            quirks.join(", ")
        }
    );

    // What the app shows; for APC units also what the generic reader would show.
    let main = decode_with(UpsHid::open_iface(i));
    let generic = (i.vid == APC_VID).then(|| generic_only(i));
    let _ = writeln!(out, "### Decoded values\n");
    match (&main, &generic) {
        (Ok((reader, s)), Some(Ok((_, g)))) => {
            let _ = writeln!(out, "| Field | {reader} (used) | generic HID |\n|---|---|---|");
            for ((label, a), (_, b)) in rows(s).into_iter().zip(rows(g)) {
                let _ = writeln!(out, "| {label} | {a} | {b} |");
            }
        }
        (Ok((reader, s)), _) => {
            let _ = writeln!(out, "| Field | {reader} |\n|---|---|");
            for (label, a) in rows(s) {
                let _ = writeln!(out, "| {label} | {a} |");
            }
            if let Some(Err(e)) = &generic {
                let _ = writeln!(out, "\nGeneric reader failed: {e}");
            }
        }
        (Err(e), _) => {
            let _ = writeln!(out, "Reading failed: {e}");
        }
    }

    if let Ok(g) = GenericHid::open_iface(i) {
        let _ = writeln!(out, "\n### Generic reader mapping\n\n```text");
        for (t, p) in g.mapping() {
            let _ = writeln!(out, "{t:<17} <- {p}");
        }
        let _ = writeln!(out, "```");
    }

    let c = &d.layout.caps;
    let _ = writeln!(
        out,
        "\n### Report layout and values\n\nReport bytes: input {}, feature {}. Value caps: input {}, feature {}. Flags: input {}, feature {}. Collections: {}.\n`*` = changed between the two samples ({} s apart).\n\n```text",
        c.InputReportByteLength,
        c.FeatureReportByteLength,
        c.NumberInputValueCaps,
        c.NumberFeatureValueCaps,
        c.NumberInputButtonCaps,
        c.NumberFeatureButtonCaps,
        c.NumberLinkCollectionNodes,
        SAMPLE_GAP.as_secs()
    );
    let _ = writeln!(
        out,
        "{:<7} {:>4}  {:<58} {:>4} {:>13} {:>4}  {:>10} {:>10}   value",
        "type", "id", "usage path", "bits", "logical", "exp", "sample 1", "sample 2"
    );
    let raw = |samples: &Samples, f: &crate::hidmap::Field| -> Option<i64> {
        match samples.get(&(f.kind, f.report_id)) {
            Some(Ok(buf)) => d.layout.value(f, &mut buf.clone()),
            _ => None,
        }
    };
    let err = |samples: &Samples, f: &crate::hidmap::Field| match samples.get(&(f.kind, f.report_id)) {
        Some(Err(code)) => format!("err {code}"),
        _ => "?".to_string(),
    };
    for f in &d.layout.fields {
        let (a, b) = (raw(&d.first, f), raw(second, f));
        let show = |v: Option<i64>, s: &Samples| v.map_or_else(|| err(s, f), |v| v.to_string());
        let value = match b {
            Some(r) if !f.flag => format!("{:.3} {}", f.scaled(r), f.unit_name()).trim_end().to_string(),
            _ => String::new(),
        };
        let changed = if a.is_some() && b.is_some() && a != b { "*" } else { " " };
        let range = if f.flag {
            "flag".to_string()
        } else {
            format!("{}..{}", f.logical_min, f.logical_max)
        };
        let exp = if f.units != 0 || f.exponent != 0 {
            f.exponent.to_string()
        } else {
            String::new()
        };
        let kind = match f.kind {
            Kind::Input => "input",
            Kind::Feature => "feature",
        };
        let _ = writeln!(
            out,
            "{kind:<7} 0x{:02X}  {:<58} {:>4} {range:>13} {exp:>4}  {:>10} {:>10}{changed}  {value}",
            f.report_id,
            f.path,
            f.bits,
            show(a, &d.first),
            show(b, second)
        );
    }
    let _ = writeln!(out, "```\n");
}

/// Build the report. Takes a few seconds (two samples per UPS).
/// `windows` and `local_time` are supplied by the caller.
pub fn generate(app_version: &str, windows: &str, local_time: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Small UPS Monitor — UPS report\n");
    let _ = writeln!(out, "- App version: {app_version}");
    if !windows.is_empty() {
        let _ = writeln!(out, "- Windows: {windows}");
    }
    if !local_time.is_empty() {
        let _ = writeln!(out, "- Created: {local_time}");
    }

    let list = ups_interfaces();
    let _ = writeln!(out, "- HID UPSes found: {}\n", list.len());
    if list.is_empty() {
        let _ = writeln!(
            out,
            "No USB HID UPS was found.\n\n\
             - Check that the UPS's USB cable is connected to this PC (not only its power cable).\n\
             - In Device Manager, a HID UPS appears under **Batteries** as \"HID UPS Battery\" (or similar).\n\
             - Some UPSes use a USB-to-serial interface or a vendor protocol instead of HID; those are not supported yet.\n\
               If yours shows up in Device Manager under **Ports (COM & LPT)** or as an unknown device, mention that in the issue."
        );
        return out;
    }

    let devices: Vec<Device> = list
        .into_iter()
        .filter_map(|iface| {
            let handle = open_path(&iface.path)?;
            let layout = Layout::load(&handle)?;
            let mut d = Device {
                iface,
                handle,
                layout,
                first: Samples::new(),
            };
            d.first = sample(&d);
            Some(d)
        })
        .collect();
    sleep(SAMPLE_GAP);
    for (n, d) in devices.iter().enumerate() {
        let second = sample(d);
        write_device(&mut out, n + 1, d, &second);
    }
    out
}
