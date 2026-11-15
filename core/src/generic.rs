//! Generic reader for any HID Power Device UPS.
//!
//! Fields are located by their standard usage paths (HID Power Device and
//! Battery System pages) in the device's own descriptor, so the report IDs and
//! bit positions don't matter. Vendor-specific data is not interpreted; that is
//! what model-specific readers (like the APC one) are for.

use std::collections::BTreeMap;

use crate::Snapshot;
use crate::decode::hid_date;
use crate::hid_win32::{Handle, UpsInterface, open_path};
use crate::hidmap::{Field, Kind, Layout};
use crate::poller::{DeviceInfo, Quirks};

/// Model limitations seen on real units.
///
/// Eaton 3S (MGE vendor ID 0463): `PercentLoad` stays 0 on mains and on
/// battery whatever is connected, and `RunTimeToEmpty` is just charge x a fixed
/// time (3000 s on a 3S 700). Tested 2026-11-15; NUT users report the same
/// (`ups.load: 0`, fixed `battery.runtime`).
pub fn quirks(vid: u16, product: &str) -> Quirks {
    let eaton_3s = vid == 0x0463 && product.trim().starts_with("Eaton 3S");
    Quirks {
        load_not_measured: eaton_3s,
        runtime_from_charge: eaton_3s,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    AcPresent,
    Charging,
    Discharging,
    LowBattery,
    ShutdownImminent,
    NeedReplacement,
    Overload,
    Charge,
    Runtime,
    BatteryV,
    InputV,
    Load,
    // slow
    BatteryVNominal,
    InputVNominal,
    TransferLow,
    TransferHigh,
    RatedW,
    RatedVa,
    Beeper,
    Test,
    // static
    MfrDate,
    BatteryMfrDate,
}

impl Target {
    const ALL: [Target; 22] = [
        Target::AcPresent,
        Target::Charging,
        Target::Discharging,
        Target::LowBattery,
        Target::ShutdownImminent,
        Target::NeedReplacement,
        Target::Overload,
        Target::Charge,
        Target::Runtime,
        Target::BatteryV,
        Target::InputV,
        Target::Load,
        Target::BatteryVNominal,
        Target::InputVNominal,
        Target::TransferLow,
        Target::TransferHigh,
        Target::RatedW,
        Target::RatedVa,
        Target::Beeper,
        Target::Test,
        Target::MfrDate,
        Target::BatteryMfrDate,
    ];

    fn is_fast(self) -> bool {
        self <= Target::Load
    }

    fn is_static(self) -> bool {
        self >= Target::MfrDate
    }

    /// Usage-path suffixes to look for, most specific first.
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Target::AcPresent => &[
                "PowerSummary.PresentStatus.ACPresent",
                "PowerSummary.ACPresent",
                ".ACPresent",
            ],
            Target::Charging => &[
                "PowerSummary.PresentStatus.Charging",
                "PowerSummary.Charging",
                ".Charging",
            ],
            Target::Discharging => &[
                "PowerSummary.PresentStatus.Discharging",
                "PowerSummary.Discharging",
                ".Discharging",
            ],
            Target::LowBattery => &[
                "PowerSummary.PresentStatus.BelowRemainingCapacityLimit",
                ".BelowRemainingCapacityLimit",
            ],
            Target::ShutdownImminent => &["PowerSummary.PresentStatus.ShutdownImminent", ".ShutdownImminent"],
            Target::NeedReplacement => &["PowerSummary.PresentStatus.NeedReplacement", ".NeedReplacement"],
            Target::Overload => &["PowerSummary.PresentStatus.Overload", ".Overload"],
            Target::Charge => &["PowerSummary.RemainingCapacity", "Battery.RemainingCapacity"],
            Target::Runtime => &["PowerSummary.RunTimeToEmpty", "Battery.RunTimeToEmpty"],
            Target::BatteryV => &["Battery.Voltage", "PowerSummary.Voltage"],
            Target::InputV => &["Input.Voltage"],
            Target::Load => &["PowerSummary.PercentLoad", "Output.PercentLoad", ".PercentLoad"],
            Target::BatteryVNominal => &["Battery.ConfigVoltage", "PowerSummary.ConfigVoltage"],
            Target::InputVNominal => &["Input.ConfigVoltage", "Flow.ConfigVoltage"],
            Target::TransferLow => &[".LowVoltageTransfer"],
            Target::TransferHigh => &[".HighVoltageTransfer"],
            Target::RatedW => &["PowerSummary.ConfigActivePower", ".ConfigActivePower"],
            Target::RatedVa => &[
                "Flow.ConfigApparentPower",
                "PowerSummary.ConfigApparentPower",
                "Output.ConfigApparentPower",
                ".ConfigApparentPower",
            ],
            Target::Beeper => &["PowerSummary.AudibleAlarmControl", ".AudibleAlarmControl"],
            Target::Test => &["Battery.Test", ".Test"],
            Target::MfrDate => &["PowerSummary.ManufacturerDate"],
            Target::BatteryMfrDate => &["Battery.ManufacturerDate"],
        }
    }
}

/// Find the field for a target: first matching candidate, feature reports
/// preferred (input reports can't always be fetched on demand).
fn pick(fields: &[Field], target: Target) -> Option<usize> {
    for suffix in target.candidates() {
        let matching = || fields.iter().enumerate().filter(|(_, f)| f.path.ends_with(suffix));
        if let Some((i, _)) = matching()
            .find(|(_, f)| f.kind == Kind::Feature)
            .or_else(|| matching().next())
        {
            return Some(i);
        }
    }
    None
}

pub struct GenericHid {
    handle: Handle,
    layout: Layout,
    map: Vec<(Target, usize)>,
    info: DeviceInfo,
}

impl GenericHid {
    pub fn open_iface(iface: &UpsInterface) -> Result<Self, String> {
        let handle = open_path(&iface.path).ok_or("cannot open UPS")?;
        let layout = Layout::load(&handle).ok_or("cannot read UPS descriptor")?;
        let info = iface.info("generic HID");
        let map: Vec<(Target, usize)> = Target::ALL
            .iter()
            .filter(|&&t| !(t == Target::Load && info.quirks.load_not_measured))
            .filter_map(|&t| pick(&layout.fields, t).map(|i| (t, i)))
            .collect();
        if !map.iter().any(|(t, _)| matches!(t, Target::Charge | Target::AcPresent)) {
            return Err(format!("{} does not report charge or AC status", iface.product));
        }
        Ok(Self {
            handle,
            layout,
            map,
            info,
        })
    }

    /// The usage paths this device mapped to (for diagnostics).
    pub fn mapping(&self) -> Vec<(String, String)> {
        self.map
            .iter()
            .map(|(t, i)| (format!("{t:?}"), self.layout.fields[*i].path.clone()))
            .collect()
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn read(&mut self, snap: &mut Snapshot, slow: bool, statics: bool) -> Result<(), String> {
        let wanted: Vec<(Target, usize)> = self
            .map
            .iter()
            .copied()
            .filter(|(t, _)| t.is_fast() || (statics && t.is_static()) || (slow && !t.is_static()))
            .collect();

        // Fetch each report once.
        let mut reports: BTreeMap<(Kind, u8), Result<Vec<u8>, u32>> = BTreeMap::new();
        for (_, i) in &wanted {
            let f = &self.layout.fields[*i];
            reports
                .entry((f.kind, f.report_id))
                .or_insert_with(|| self.layout.fetch(&self.handle, f.kind, f.report_id));
        }

        let mut fast_ok = 0;
        let mut last_err = 0;
        for (t, i) in wanted {
            let f = &self.layout.fields[i];
            let raw = match reports.get_mut(&(f.kind, f.report_id)) {
                Some(Ok(buf)) => self.layout.value(f, buf),
                Some(Err(code)) => {
                    last_err = *code;
                    None
                }
                None => None,
            };
            let Some(raw) = raw else { continue };
            fast_ok += usize::from(t.is_fast());
            let v = f.scaled(raw);
            let flag = raw != 0;
            let u16v = || Some(v.round().clamp(0.0, 65535.0) as u16);
            let u8v = || Some(v.round().clamp(0.0, 255.0) as u8);
            match t {
                Target::AcPresent => snap.ac_present = Some(flag),
                Target::Charging => snap.charging = Some(flag),
                Target::Discharging => snap.discharging = Some(flag),
                Target::LowBattery => snap.low_battery = Some(flag),
                Target::ShutdownImminent => snap.shutdown_imminent = Some(flag),
                Target::NeedReplacement => snap.need_replacement = Some(flag),
                Target::Overload => snap.overload = Some(flag),
                Target::Charge => snap.charge = u8v().map(|c| c.min(100)),
                Target::Runtime => snap.runtime_s = Some(v.max(0.0) as u32),
                Target::BatteryV => snap.battery_v = Some(v as f32),
                Target::InputV => snap.input_v = u16v(),
                Target::Load => snap.load_pct = u8v(),
                Target::BatteryVNominal => snap.battery_v_nominal = Some(v as f32),
                Target::InputVNominal => snap.input_v_nominal = u16v(),
                Target::TransferLow => snap.transfer_low = u16v(),
                Target::TransferHigh => snap.transfer_high = u16v(),
                Target::RatedW => snap.rated_w = u16v().filter(|&w| w > 0),
                Target::RatedVa => snap.rated_va = u16v().filter(|&w| w > 0),
                Target::Beeper => snap.beeper = u8v(),
                Target::Test => snap.test_result = u8v(),
                Target::MfrDate => snap.mfr_date = hid_date(raw as u32),
                Target::BatteryMfrDate => snap.battery_mfr_date = hid_date(raw as u32),
            }
        }
        if fast_ok == 0 {
            Err(format!("no data (error {last_err})"))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::quirks;

    #[test]
    fn eaton_3s_load_and_runtime_quirks() {
        let q = quirks(0x0463, "Eaton 3S");
        assert!(q.load_not_measured && q.runtime_from_charge);
        assert_eq!(quirks(0x0463, "Eaton 5E"), Default::default());
        assert_eq!(
            quirks(0x051D, "Back-UPS BR1500GI FW:878.L3 .I USB FW:L3"),
            Default::default()
        );
    }
}
