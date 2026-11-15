//! Descriptor-driven view of any HID Power Device.
//!
//! Windows parses each device's report descriptor into "preparsed data". From
//! it we learn, for every usage (e.g. `UPS.PowerSummary.RemainingCapacity`),
//! which report carries it, where, and with what unit scaling. That lets us
//! read UPSes whose report layout differs from the APC one in decode.rs.

use std::collections::BTreeSet;
use std::mem::zeroed;

use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_LINK_COLLECTION_NODE, HIDP_REPORT_TYPE, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS,
    HidD_FreePreparsedData, HidD_GetFeature, HidD_GetInputReport, HidD_GetPreparsedData, HidP_Feature,
    HidP_GetButtonCaps, HidP_GetCaps, HidP_GetLinkCollectionNodes, HidP_GetUsageValue, HidP_GetUsages,
    HidP_GetValueCaps, HidP_Input, PHIDP_PREPARSED_DATA,
};

use crate::hid_win32::Handle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Input,
    Feature,
}

impl Kind {
    fn raw(self) -> HIDP_REPORT_TYPE {
        match self {
            Kind::Input => HidP_Input,
            Kind::Feature => HidP_Feature,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Field {
    pub kind: Kind,
    pub report_id: u8,
    pub page: u16,
    pub usage: u16,
    /// Link collection index (needed to tell e.g. Input.Voltage from Output.Voltage).
    pub link: u16,
    /// Dotted NUT-style path, e.g. "UPS.PowerSummary.RemainingCapacity".
    pub path: String,
    pub bits: u16,
    pub logical_min: i32,
    pub logical_max: i32,
    pub units: u32,
    pub exponent: i8,
    /// 1-bit flag (Windows reports these as "buttons").
    pub flag: bool,
}

impl Field {
    /// Raw value converted to SI units (V, W, A, Hz, s, %…).
    pub fn scaled(&self, raw: i64) -> f64 {
        // HID uses cgs-based units: volt and watt carry an extra 10^7.
        let extra = match self.units {
            0x00F0_D121 | 0x0000_D121 => -7,
            _ => 0,
        };
        raw as f64 * 10f64.powi(i32::from(self.exponent) + extra)
    }

    pub fn unit_name(&self) -> &'static str {
        match self.units {
            0x00F0_D121 => "V",
            0x0000_D121 => "W",
            0x0010_0001 => "A",
            0x0000_F001 => "Hz",
            0x0000_1001 => "s",
            0x0001_0001 => "K",
            _ => "",
        }
    }
}

pub struct Layout {
    pp: PHIDP_PREPARSED_DATA,
    pub caps: HIDP_CAPS,
    pub fields: Vec<Field>,
}

impl Drop for Layout {
    fn drop(&mut self) {
        unsafe { HidD_FreePreparsedData(self.pp) };
    }
}

// Preparsed data is plain memory owned by this struct.
unsafe impl Send for Layout {}

/// Names for the usages that matter on UPSes (HID Power Device / Battery System pages).
pub fn usage_name(page: u16, usage: u16) -> Option<&'static str> {
    Some(match (page, usage) {
        (0x84, 0x01) => "iName",
        (0x84, 0x02) => "PresentStatus",
        (0x84, 0x03) => "ChangedStatus",
        (0x84, 0x04) => "UPS",
        (0x84, 0x05) => "PowerSupply",
        (0x84, 0x10) => "BatterySystem",
        (0x84, 0x12) => "Battery",
        (0x84, 0x14) => "Charger",
        (0x84, 0x16) => "PowerConverter",
        (0x84, 0x18) => "OutletSystem",
        (0x84, 0x1A) => "Input",
        (0x84, 0x1C) => "Output",
        (0x84, 0x1E) => "Flow",
        (0x84, 0x20) => "Outlet",
        (0x84, 0x24) => "PowerSummary",
        (0x84, 0x11) => "BatterySystemID",
        (0x84, 0x13) => "BatteryID",
        (0x84, 0x15) => "ChargerID",
        (0x84, 0x17) => "PowerConverterID",
        (0x84, 0x19) => "OutletSystemID",
        (0x84, 0x1B) => "InputID",
        (0x84, 0x1D) => "OutputID",
        (0x84, 0x1F) => "FlowID",
        (0x84, 0x21) => "OutletID",
        (0x84, 0x23) => "GangID",
        (0x84, 0x25) => "PowerSummaryID",
        (0x84, 0x30) => "Voltage",
        (0x84, 0x31) => "Current",
        (0x84, 0x32) => "Frequency",
        (0x84, 0x33) => "ApparentPower",
        (0x84, 0x34) => "ActivePower",
        (0x84, 0x35) => "PercentLoad",
        (0x84, 0x36) => "Temperature",
        (0x84, 0x40) => "ConfigVoltage",
        (0x84, 0x41) => "ConfigCurrent",
        (0x84, 0x42) => "ConfigFrequency",
        (0x84, 0x43) => "ConfigApparentPower",
        (0x84, 0x44) => "ConfigActivePower",
        (0x84, 0x45) => "ConfigPercentLoad",
        (0x84, 0x50) => "SwitchOnControl",
        (0x84, 0x51) => "SwitchOffControl",
        (0x84, 0x52) => "ToggleControl",
        (0x84, 0x53) => "LowVoltageTransfer",
        (0x84, 0x54) => "HighVoltageTransfer",
        (0x84, 0x55) => "DelayBeforeReboot",
        (0x84, 0x56) => "DelayBeforeStartup",
        (0x84, 0x57) => "DelayBeforeShutdown",
        (0x84, 0x58) => "Test",
        (0x84, 0x5A) => "AudibleAlarmControl",
        (0x84, 0x60) => "Present",
        (0x84, 0x61) => "Good",
        (0x84, 0x62) => "InternalFailure",
        (0x84, 0x63) => "VoltageOutOfRange",
        (0x84, 0x64) => "FrequencyOutOfRange",
        (0x84, 0x65) => "Overload",
        (0x84, 0x66) => "OverCharged",
        (0x84, 0x67) => "OverTemperature",
        (0x84, 0x68) => "ShutdownRequested",
        (0x84, 0x69) => "ShutdownImminent",
        (0x84, 0x6B) => "SwitchOnOff",
        (0x84, 0x6C) => "Switchable",
        (0x84, 0x6D) => "Used",
        (0x84, 0x6E) => "Boost",
        (0x84, 0x6F) => "Buck",
        (0x84, 0x70) => "Initialized",
        (0x84, 0x71) => "Tested",
        (0x84, 0x72) => "AwaitingPower",
        (0x84, 0x73) => "CommunicationLost",
        (0x84, 0xFD) => "iManufacturer",
        (0x84, 0xFE) => "iProduct",
        (0x84, 0xFF) => "iSerialNumber",
        (0x85, 0x29) => "RemainingCapacityLimit",
        (0x85, 0x2A) => "RemainingTimeLimit",
        (0x85, 0x2C) => "CapacityMode",
        (0x85, 0x42) => "BelowRemainingCapacityLimit",
        (0x85, 0x43) => "RemainingTimeLimitExpired",
        (0x85, 0x44) => "Charging",
        (0x85, 0x45) => "Discharging",
        (0x85, 0x46) => "FullyCharged",
        (0x85, 0x47) => "FullyDischarged",
        (0x85, 0x4B) => "NeedReplacement",
        (0x85, 0x65) => "AbsoluteStateOfCharge",
        (0x85, 0x66) => "RemainingCapacity",
        (0x85, 0x67) => "FullChargeCapacity",
        (0x85, 0x68) => "RunTimeToEmpty",
        (0x85, 0x69) => "AverageTimeToEmpty",
        (0x85, 0x6A) => "AverageTimeToFull",
        (0x85, 0x6B) => "CycleCount",
        (0x85, 0x83) => "DesignCapacity",
        (0x85, 0x85) => "ManufacturerDate",
        (0x85, 0x89) => "iDeviceChemistry",
        (0x85, 0x8B) => "Rechargeable",
        (0x85, 0x8C) => "WarningCapacityLimit",
        (0x85, 0x8D) => "CapacityGranularity1",
        (0x85, 0x8E) => "CapacityGranularity2",
        (0x85, 0x8F) => "iOEMInformation",
        (0x85, 0xD0) => "ACPresent",
        (0x85, 0xD1) => "BatteryPresent",
        (0x85, 0xD2) => "PowerFail",
        (0x85, 0xD3) => "AlarmInhibited",
        (0x85, 0xDB) => "VoltageNotRegulated",
        (0xFF86, 0x16) => "APCBattReplaceDate",
        (0xFF86, 0x42) => "APC_UPS_FirmwareRevision",
        (0xFF86, 0x52) => "APCLineFailCause",
        (0xFF86, 0x60) => "APCStatusFlag",
        (0xFF86, 0x61) => "APCSensitivity",
        (0xFF86, 0x72) => "APCPanelTest",
        (0xFF86, 0x7C) => "APCDelayBeforeReboot",
        (0xFF86, 0x7D) => "APCDelayBeforeShutdown",
        (0xFF86, 0x05) => "APCGeneralCollection",
        _ => return None,
    })
}

fn name(page: u16, usage: u16) -> String {
    usage_name(page, usage).map_or_else(|| format!("{page:04x}{usage:04x}"), str::to_string)
}

fn exponent(units_exp: u32) -> i8 {
    let e = (units_exp & 0xF) as i8;
    if e > 7 { e - 16 } else { e }
}

impl Layout {
    pub fn load(h: &Handle) -> Option<Layout> {
        unsafe {
            let mut pp = zeroed();
            if !HidD_GetPreparsedData(h.0, &mut pp) {
                return None;
            }
            let mut caps: HIDP_CAPS = zeroed();
            if HidP_GetCaps(pp, &mut caps) != HIDP_STATUS_SUCCESS {
                HidD_FreePreparsedData(pp);
                return None;
            }

            // Collection paths, indexed by link collection number.
            let mut n_links = u32::from(caps.NumberLinkCollectionNodes);
            let mut nodes: Vec<HIDP_LINK_COLLECTION_NODE> = vec![zeroed(); n_links as usize];
            HidP_GetLinkCollectionNodes(nodes.as_mut_ptr(), &mut n_links, pp);
            nodes.truncate(n_links as usize);
            let paths: Vec<String> = (0..nodes.len())
                .map(|i| {
                    let mut parts = Vec::new();
                    let mut j = i;
                    for _ in 0..32 {
                        let n = &nodes[j];
                        parts.push(name(n.LinkUsagePage, n.LinkUsage));
                        if j == 0 {
                            break;
                        }
                        j = usize::from(n.Parent);
                    }
                    parts.reverse();
                    parts.join(".")
                })
                .collect();
            let path_of = |link: u16| paths.get(usize::from(link)).cloned().unwrap_or_default();

            let mut fields = Vec::new();
            for (kind, n_values, n_buttons) in [
                (Kind::Input, caps.NumberInputValueCaps, caps.NumberInputButtonCaps),
                (Kind::Feature, caps.NumberFeatureValueCaps, caps.NumberFeatureButtonCaps),
            ] {
                let mut n = n_values;
                let mut vcaps: Vec<HIDP_VALUE_CAPS> = vec![zeroed(); usize::from(n)];
                if n > 0 && HidP_GetValueCaps(kind.raw(), vcaps.as_mut_ptr(), &mut n, pp) == HIDP_STATUS_SUCCESS {
                    for c in &vcaps[..usize::from(n)] {
                        let (lo, hi) = if c.IsRange {
                            (c.Anonymous.Range.UsageMin, c.Anonymous.Range.UsageMax)
                        } else {
                            (c.Anonymous.NotRange.Usage, c.Anonymous.NotRange.Usage)
                        };
                        for usage in lo..=hi {
                            fields.push(Field {
                                kind,
                                report_id: c.ReportID,
                                page: c.UsagePage,
                                usage,
                                link: c.LinkCollection,
                                path: format!("{}.{}", path_of(c.LinkCollection), name(c.UsagePage, usage)),
                                bits: c.BitSize,
                                logical_min: c.LogicalMin,
                                logical_max: c.LogicalMax,
                                units: c.Units,
                                exponent: exponent(c.UnitsExp),
                                flag: false,
                            });
                        }
                    }
                }
                let mut n = n_buttons;
                let mut bcaps: Vec<HIDP_BUTTON_CAPS> = vec![zeroed(); usize::from(n)];
                if n > 0 && HidP_GetButtonCaps(kind.raw(), bcaps.as_mut_ptr(), &mut n, pp) == HIDP_STATUS_SUCCESS {
                    for c in &bcaps[..usize::from(n)] {
                        let (lo, hi) = if c.IsRange {
                            (c.Anonymous.Range.UsageMin, c.Anonymous.Range.UsageMax)
                        } else {
                            (c.Anonymous.NotRange.Usage, c.Anonymous.NotRange.Usage)
                        };
                        for usage in lo..=hi {
                            fields.push(Field {
                                kind,
                                report_id: c.ReportID,
                                page: c.UsagePage,
                                usage,
                                link: c.LinkCollection,
                                path: format!("{}.{}", path_of(c.LinkCollection), name(c.UsagePage, usage)),
                                bits: 1,
                                logical_min: 0,
                                logical_max: 1,
                                units: 0,
                                exponent: 0,
                                flag: true,
                            });
                        }
                    }
                }
            }
            fields.sort_by(|a, b| (a.kind, a.report_id, &a.path).cmp(&(b.kind, b.report_id, &b.path)));
            Some(Layout { pp, caps, fields })
        }
    }

    pub fn report_len(&self, kind: Kind) -> usize {
        usize::from(match kind {
            Kind::Input => self.caps.InputReportByteLength,
            Kind::Feature => self.caps.FeatureReportByteLength,
        })
    }

    /// Distinct (kind, report id) pairs.
    pub fn reports(&self) -> BTreeSet<(Kind, u8)> {
        self.fields.iter().map(|f| (f.kind, f.report_id)).collect()
    }

    pub fn find(&self, path_suffix: &str) -> Vec<&Field> {
        self.fields.iter().filter(|f| f.path.ends_with(path_suffix)).collect()
    }

    /// Fetch one report from the device. Returns the buffer (byte 0 = report id).
    pub fn fetch(&self, h: &Handle, kind: Kind, id: u8) -> Result<Vec<u8>, u32> {
        let mut buf = vec![0u8; self.report_len(kind).max(2)];
        buf[0] = id;
        let ok = unsafe {
            match kind {
                Kind::Feature => HidD_GetFeature(h.0, buf.as_mut_ptr().cast(), buf.len() as u32),
                Kind::Input => HidD_GetInputReport(h.0, buf.as_mut_ptr().cast(), buf.len() as u32),
            }
        };
        if ok {
            Ok(buf)
        } else {
            Err(unsafe { windows_sys::Win32::Foundation::GetLastError() })
        }
    }

    /// Extract a field's raw value from a fetched report (sign-extended when the
    /// logical range is signed).
    pub fn value(&self, f: &Field, report: &mut [u8]) -> Option<i64> {
        unsafe {
            if f.flag {
                let mut list = [0u16; 64];
                let mut n = list.len() as u32;
                let st = HidP_GetUsages(
                    f.kind.raw(),
                    f.page,
                    f.link,
                    list.as_mut_ptr(),
                    &mut n,
                    self.pp,
                    report.as_mut_ptr(),
                    report.len() as u32,
                );
                (st == HIDP_STATUS_SUCCESS).then(|| i64::from(list[..n as usize].contains(&f.usage)))
            } else {
                let mut v = 0u32;
                let st = HidP_GetUsageValue(
                    f.kind.raw(),
                    f.page,
                    f.link,
                    f.usage,
                    &mut v,
                    self.pp,
                    report.as_ptr(),
                    report.len() as u32,
                );
                if st != HIDP_STATUS_SUCCESS {
                    return None;
                }
                let bits = u32::from(f.bits).clamp(1, 32);
                let raw = i64::from(v);
                if f.logical_min < 0 && bits < 64 && raw >= 1 << (bits - 1) {
                    Some(raw - (1 << bits))
                } else {
                    Some(raw)
                }
            }
        }
    }
}
