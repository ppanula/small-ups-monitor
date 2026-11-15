//! Append-only CSV log of power events in %APPDATA%\Small UPS Monitor\events.csv.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::settings::data_dir;

pub struct EventLog {
    path: Option<PathBuf>,
}

/// The most recent switch to battery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outage {
    /// Local time, "YYYY-MM-DD HH:MM:SS".
    pub start: String,
    /// Seconds on battery; None while it is still going on (or the restore was not seen).
    pub secs: Option<u64>,
}

pub fn local_timestamp() -> String {
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

/// "YYYY-MM-DD HH:MM:SS" -> seconds since 1970 (calendar math only; the
/// timestamps are local time, which is fine for differences).
fn to_seconds(ts: &str) -> Option<i64> {
    let (date, time) = ts.split_once(' ')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|p| p.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    // Days from civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// The UPS id in a log line's last column, if the line has one
/// (`time,event,"detail",ups`).
fn line_ups(rest: &str) -> Option<&str> {
    let i = rest.rfind("\",")?;
    Some(rest[i + 2..].trim())
}

/// Find the last outage of UPS `ups` in CSV text. Lines written before the
/// log had a UPS column only ever came from the APC reader, so they count for
/// APC devices.
pub fn parse_last_outage(csv: &str, ups: &str) -> Option<Outage> {
    let legacy_ok = ups.starts_with("051D:") || ups == "DEMO";
    let mut last: Option<Outage> = None;
    for line in csv.lines().skip(1) {
        let mut cols = line.splitn(3, ',');
        let (Some(ts), Some(event)) = (cols.next(), cols.next()) else {
            continue;
        };
        let belongs = match line_ups(cols.next().unwrap_or("")) {
            Some(id) => id == ups,
            None => legacy_ok,
        };
        if !belongs {
            continue;
        }
        match event {
            "power_failure" => {
                last = Some(Outage {
                    start: ts.to_string(),
                    secs: None,
                })
            }
            "power_restored" => {
                if let Some(o) = last.as_mut().filter(|o| o.secs.is_none()) {
                    o.secs = to_seconds(ts)
                        .zip(to_seconds(&o.start))
                        .map(|(b, a)| (b - a).max(0) as u64);
                }
            }
            _ => {}
        }
    }
    last
}

impl EventLog {
    pub fn new(demo: bool) -> Self {
        let name = if demo { "events-demo.csv" } else { "events.csv" };
        Self {
            path: data_dir().map(|d| d.join(name)),
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    pub fn last_outage(&self, ups: &str) -> Option<Outage> {
        parse_last_outage(&fs::read_to_string(self.path.as_ref()?).ok()?, ups)
    }

    /// Append `time,event,"detail",ups`.
    pub fn write(&self, event: &str, detail: &str, ups: &str) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let new = !path.exists();
        let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        if new {
            let _ = writeln!(f, "time,event,detail,ups");
        }
        let detail = detail.replace('"', "'");
        let ups = ups.replace([',', '"'], "_");
        let _ = writeln!(f, "{},{event},\"{detail}\",{ups}", local_timestamp());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APC: &str = "051D:0002:3B0000X00000";
    const EATON: &str = "0463:FFFF:000000000";

    #[test]
    fn finds_last_outage_and_duration_in_legacy_lines() {
        let csv = "time,event,detail\n\
            2026-11-15 11:44:12,ups_connected,\"x\"\n\
            2026-11-15 11:50:41,power_failure,\"100% charge\"\n\
            2026-11-15 11:51:09,power_restored,\"on battery after 28 s\"\n";
        assert_eq!(
            parse_last_outage(csv, APC),
            Some(Outage {
                start: "2026-11-15 11:50:41".into(),
                secs: Some(28)
            })
        );
        // Lines without a UPS column were written by the APC-only version.
        assert_eq!(parse_last_outage(csv, EATON), None);
    }

    #[test]
    fn outages_are_kept_per_ups() {
        let csv = "time,event,detail\n\
            2026-11-15 11:50:41,power_failure,\"a, b\"\n\
            2026-11-15 11:51:09,power_restored,\"c\"\n\
            2026-11-16 10:00:00,power_failure,\"x\",0463:FFFF:000000000\n\
            2026-11-16 10:00:05,power_failure,\"y\",051D:0002:3B0000X00000\n\
            2026-11-16 10:01:05,power_restored,\"z\",051D:0002:3B0000X00000\n";
        assert_eq!(
            parse_last_outage(csv, APC),
            Some(Outage {
                start: "2026-11-16 10:00:05".into(),
                secs: Some(60)
            })
        );
        assert_eq!(
            parse_last_outage(csv, EATON),
            Some(Outage {
                start: "2026-11-16 10:00:00".into(),
                secs: None
            })
        );
    }

    #[test]
    fn duration_across_midnight_and_ongoing() {
        let csv = "time,event,detail,ups\n\
            2026-12-31 23:59:50,power_failure,\"\",DEMO\n\
            2027-01-01 00:00:20,power_restored,\"\",DEMO\n\
            2027-01-02 08:00:00,power_failure,\"\",DEMO\n";
        assert_eq!(
            parse_last_outage(csv, "DEMO"),
            Some(Outage {
                start: "2027-01-02 08:00:00".into(),
                secs: None
            })
        );
        let done = &csv[..csv.rfind("2027-01-02").unwrap()];
        assert_eq!(parse_last_outage(done, "DEMO").unwrap().secs, Some(30));
    }

    #[test]
    fn no_outage() {
        assert_eq!(
            parse_last_outage("time,event,detail\n2026-11-15 11:44:12,ups_connected,\"x\"\n", APC),
            None
        );
    }
}
