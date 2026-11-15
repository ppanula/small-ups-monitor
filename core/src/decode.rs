//! Raw HID report bytes -> normalized snapshot.
//!
//! Report IDs and scaling come from docs/APC-UPS-GUIDE.md (verified on a BR1500GI).
//! Every report is `[report_id, value bytes...]`, little-endian.

use std::fmt;
use std::time::SystemTime;

/// Polled every cycle (~2 s): status, charge/runtime, load, input and battery voltage.
pub const FAST: &[u8] = &[0x06, 0x0C, 0x13, 0x14, 0x50, 0x31, 0x09];
/// Polled every ~30 s: nominal values, thresholds, settings, last transfer, test result.
pub const SLOW: &[u8] = &[0x08, 0x30, 0x32, 0x33, 0x35, 0x36, 0x52, 0x18, 0x21];
/// Read on connect and every ~10 min: dates.
pub const STATIC: &[u8] = &[0x07, 0x1C, 0x20];

/// One poll cycle for APC-layout devices (the APC reader and `--demo`):
/// fetch the report IDs above and decode them into `snap`.
pub fn read_apc_reports(
    snap: &mut Snapshot,
    slow: bool,
    statics: bool,
    mut fetch: impl FnMut(u8) -> Result<crate::Report, String>,
) -> Result<(), String> {
    let mut ids: Vec<u8> = FAST.to_vec();
    if slow {
        ids.extend_from_slice(SLOW);
    }
    if statics {
        ids.extend_from_slice(STATIC);
    }
    let mut fast_ok = 0;
    let mut err = String::new();
    for id in ids {
        match fetch(id) {
            Ok(r) => {
                snap.apply(&r);
                fast_ok += usize::from(FAST.contains(&id));
            }
            Err(e) => err = e,
        }
    }
    if fast_ok == 0 { Err(err) } else { Ok(()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub y: u16,
    pub m: u8,
    pub d: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

/// HID Power Device packed date (`ManufacturerDate`).
pub fn hid_date(v: u32) -> Option<Date> {
    if v == 0 {
        return None;
    }
    Some(Date {
        y: 1980 + (v >> 9) as u16,
        m: ((v >> 5) & 0x0F) as u8,
        d: (v & 0x1F) as u8,
    })
}

/// APC "hex as decimal" date (`APCBattReplaceDate`), e.g. 0x092501 -> 2001-09-25.
pub fn apc_date(v: u32) -> Option<Date> {
    if v == 0 {
        return None;
    }
    let bcd = |shift: u32| ((v >> shift) & 0xF) + 10 * ((v >> (shift + 4)) & 0xF);
    let yy = bcd(0);
    let y = if yy >= 70 { 1900 + yy } else { 2000 + yy };
    Some(Date {
        y: y as u16,
        m: bcd(16) as u8,
        d: bcd(8) as u8,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpsState {
    Unknown,
    Online,
    Charging,
    OnBattery,
    LowBattery,
    ShutdownImminent,
}

impl UpsState {
    pub fn label(self) -> &'static str {
        match self {
            UpsState::Unknown => "UNKNOWN",
            UpsState::Online => "ONLINE",
            UpsState::Charging => "CHARGING",
            UpsState::OnBattery => "ON BATTERY",
            UpsState::LowBattery => "LOW BATTERY",
            UpsState::ShutdownImminent => "SHUTDOWN SOON",
        }
    }

    pub fn on_battery(self) -> bool {
        matches!(
            self,
            UpsState::OnBattery | UpsState::LowBattery | UpsState::ShutdownImminent
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub timestamp: Option<SystemTime>,
    pub ac_present: Option<bool>,
    pub charging: Option<bool>,
    pub discharging: Option<bool>,
    pub apc_status: Option<u8>,
    pub low_battery: Option<bool>,
    pub shutdown_imminent: Option<bool>,
    pub charge: Option<u8>,
    pub runtime_s: Option<u32>,
    pub battery_v: Option<f32>,
    pub battery_v_nominal: Option<f32>,
    pub input_v: Option<u16>,
    pub input_v_nominal: Option<u16>,
    pub transfer_low: Option<u16>,
    pub transfer_high: Option<u16>,
    pub load_pct: Option<u8>,
    pub rated_w: Option<u16>,
    /// Apparent-power rating, for UPSes that report VA but not watts.
    pub rated_va: Option<u16>,
    pub need_replacement: Option<bool>,
    pub overload: Option<bool>,
    pub beeper: Option<u8>,
    pub sensitivity: Option<u8>,
    pub line_fail_cause: Option<u8>,
    pub test_result: Option<u8>,
    pub mfr_date: Option<Date>,
    pub battery_mfr_date: Option<Date>,
    pub battery_replace_date: Option<Date>,
}

fn u8_at(r: &[u8], i: usize) -> Option<u8> {
    r.get(i).copied()
}

fn u16_at(r: &[u8], i: usize) -> Option<u16> {
    Some(u16::from(*r.get(i)?) | (u16::from(*r.get(i + 1)?) << 8))
}

fn u24_at(r: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(*r.get(i)?) | (u32::from(*r.get(i + 1)?) << 8) | (u32::from(*r.get(i + 2)?) << 16))
}

impl Snapshot {
    /// Apply one raw report (`r[0]` is the report ID) to the snapshot.
    pub fn apply(&mut self, r: &[u8]) {
        let Some(&id) = r.first() else { return };
        let b = |i| u8_at(r, i);
        let w = |i| u16_at(r, i);
        match id {
            0x06 => {
                self.charging = b(1).map(|v| v != 0);
                self.discharging = b(2).map(|v| v != 0);
                self.apc_status = b(3);
            }
            0x0C => {
                self.charge = b(1);
                self.runtime_s = w(2).map(u32::from);
            }
            0x13 => self.ac_present = b(1).map(|v| v != 0),
            0x14 => {
                self.low_battery = b(1).map(|v| v != 0);
                self.shutdown_imminent = b(2).map(|v| v != 0);
            }
            // Battery voltages: unit exponent 5 on this unit -> raw / 100.
            0x08 => self.battery_v_nominal = w(1).map(|v| f32::from(v) / 100.0),
            0x09 => self.battery_v = w(1).map(|v| f32::from(v) / 100.0),
            0x30 => self.input_v_nominal = w(1),
            0x31 => self.input_v = w(1),
            0x32 => self.transfer_low = w(1),
            0x33 => self.transfer_high = w(1),
            0x35 => self.sensitivity = b(1),
            0x36 => self.line_fail_cause = b(1),
            0x50 => self.load_pct = b(1),
            0x52 => self.rated_w = w(1),
            0x18 => self.beeper = b(1),
            0x21 => self.test_result = b(1),
            0x07 => self.mfr_date = w(1).and_then(|v| hid_date(v.into())),
            0x20 => self.battery_mfr_date = w(1).and_then(|v| hid_date(v.into())),
            0x1C => self.battery_replace_date = u24_at(r, 1).and_then(apc_date),
            _ => {}
        }
    }

    pub fn state(&self) -> UpsState {
        if self.shutdown_imminent == Some(true) {
            return UpsState::ShutdownImminent;
        }
        let on_batt = self.ac_present == Some(false) || self.discharging == Some(true);
        if on_batt {
            if self.low_battery == Some(true) {
                UpsState::LowBattery
            } else {
                UpsState::OnBattery
            }
        } else if self.charging == Some(true) {
            UpsState::Charging
        } else if self.ac_present == Some(true) {
            UpsState::Online
        } else {
            UpsState::Unknown
        }
    }

    /// NUT-style status tokens, e.g. "OL CHRG".
    pub fn status_tokens(&self) -> String {
        let mut t = Vec::new();
        match self.ac_present {
            Some(true) => t.push("OL"),
            Some(false) => t.push("OB"),
            None => {}
        }
        if self.charging == Some(true) {
            t.push("CHRG");
        }
        if self.discharging == Some(true) {
            t.push("DISCHRG");
        }
        if self.low_battery == Some(true) {
            t.push("LB");
        }
        if self.shutdown_imminent == Some(true) {
            t.push("FSD");
        }
        if self.overload == Some(true) {
            t.push("OVER");
        }
        if self.need_replacement == Some(true) {
            t.push("RB");
        }
        t.join(" ")
    }

    /// Estimated output watts = rated W x load %. Not measured by the UPS.
    pub fn est_watts(&self) -> Option<u32> {
        Some(u32::from(self.rated_w?) * u32::from(self.load_pct?) / 100)
    }

    /// Estimated output VA = rated VA x load %, for UPSes without a watt rating.
    pub fn est_va(&self) -> Option<u32> {
        Some(u32::from(self.rated_va?) * u32::from(self.load_pct?) / 100)
    }
}

pub fn beeper_str(v: u8) -> &'static str {
    match v {
        1 => "disabled",
        2 => "enabled",
        3 => "muted",
        _ => "?",
    }
}

pub fn sensitivity_str(v: u8) -> &'static str {
    match v {
        0 => "low",
        1 => "medium",
        2 => "high",
        _ => "?",
    }
}

pub fn test_str(v: u8) -> &'static str {
    match v {
        1 => "passed",
        2 => "warning",
        3 => "error",
        4 => "aborted",
        5 => "in progress",
        6 => "none",
        7 => "scheduled",
        _ => "?",
    }
}

/// apcupsd mapping; NUT only documents 1/2/4/8/9.
pub fn line_fail_str(v: u8) -> &'static str {
    match v {
        0 => "none",
        1 => "low line voltage",
        2 => "high line voltage",
        3 => "ripple",
        4 => "notch/spike",
        5 => "self test",
        6 => "forced",
        7 => "frequency out of range",
        8 => "notch or blackout",
        9 => "spike or blackout",
        10 => "accessory shutdown",
        11 => "self test",
        12 => "front button test",
        13 => "two-week self test",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raw bytes from the raw snapshot appendix of docs/APC-UPS-GUIDE.md.
    fn snapshot() -> Snapshot {
        let reports: &[&[u8]] = &[
            &[0x06, 0x00, 0x00, 0x08, 0x00],
            &[0x0C, 0x64, 0xD0, 0x0E, 0x00],
            &[0x13, 0x01, 0x00, 0x00, 0x00],
            &[0x14, 0x00, 0x00, 0x00, 0x00],
            &[0x07, 0x4D, 0x3F, 0x00, 0x00],
            &[0x08, 0x60, 0x09, 0x00, 0x00],
            &[0x09, 0xC4, 0x0A, 0x00, 0x00],
            &[0x18, 0x01, 0x00, 0x00, 0x00],
            &[0x1C, 0x01, 0x25, 0x09, 0x00],
            &[0x20, 0x7D, 0x55, 0x00, 0x00],
            &[0x30, 0xE6, 0x00, 0x00, 0x00],
            &[0x31, 0xE8, 0x00, 0x00, 0x00],
            &[0x32, 0xB0, 0x00, 0x00, 0x00],
            &[0x33, 0x26, 0x01, 0x00, 0x00],
            &[0x35, 0x01, 0x00, 0x00, 0x00],
            &[0x36, 0x03, 0x00, 0x00, 0x00],
            &[0x50, 0x26, 0x00, 0x00, 0x00],
            &[0x52, 0x61, 0x03, 0x00, 0x00],
        ];
        let mut s = Snapshot::default();
        for r in reports {
            s.apply(r);
        }
        s
    }

    #[test]
    fn decodes_guide_snapshot() {
        let s = snapshot();
        assert_eq!(s.charge, Some(100));
        assert_eq!(s.runtime_s, Some(3792));
        assert_eq!(s.ac_present, Some(true));
        assert_eq!(s.apc_status, Some(8));
        assert_eq!(s.input_v, Some(232));
        assert_eq!(s.input_v_nominal, Some(230));
        assert_eq!(s.transfer_low, Some(176));
        assert_eq!(s.transfer_high, Some(294));
        assert_eq!(s.load_pct, Some(38));
        assert_eq!(s.rated_w, Some(865));
        assert_eq!(s.est_watts(), Some(328));
        assert!((s.battery_v.unwrap() - 27.56).abs() < 0.001);
        assert!((s.battery_v_nominal.unwrap() - 24.0).abs() < 0.001);
        assert_eq!(s.mfr_date.unwrap().to_string(), "2011-10-13");
        assert_eq!(s.battery_mfr_date.unwrap().to_string(), "2022-11-29");
        assert_eq!(s.battery_replace_date.unwrap().to_string(), "2001-09-25");
        assert_eq!(s.state(), UpsState::Online);
        assert_eq!(s.status_tokens(), "OL");
    }

    #[test]
    fn on_battery_states() {
        let mut s = snapshot();
        s.apply(&[0x13, 0x00]);
        s.apply(&[0x06, 0x00, 0x01, 0x00]);
        assert_eq!(s.state(), UpsState::OnBattery);
        s.apply(&[0x14, 0x01, 0x00]);
        assert_eq!(s.state(), UpsState::LowBattery);
        assert_eq!(s.status_tokens(), "OB DISCHRG LB");
        s.apply(&[0x14, 0x01, 0x01]);
        assert_eq!(s.state(), UpsState::ShutdownImminent);
    }
}
