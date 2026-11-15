//! Simulated UPS for trying the UI without unplugging anything (`--demo`).
//!
//! Repeats a 70-second cycle:
//!   0–15 s  online
//!  15–35 s  power failure, on battery, charge falling
//!  35–45 s  low battery
//!  45–52 s  USB disconnected
//!  52–70 s  mains back, charging

use std::sync::OnceLock;
use std::time::Instant;

use crate::Snapshot;
use crate::poller::{DeviceInfo, Report, Transport};

const CYCLE: f32 = 70.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Online,
    OnBattery,
    LowBattery,
    Unplugged,
    Charging,
}

fn clock() -> (Phase, f32) {
    static START: OnceLock<Instant> = OnceLock::new();
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f32() % CYCLE;
    let phase = match t {
        t if t < 15.0 => Phase::Online,
        t if t < 35.0 => Phase::OnBattery,
        t if t < 45.0 => Phase::LowBattery,
        t if t < 52.0 => Phase::Unplugged,
        _ => Phase::Charging,
    };
    (phase, t)
}

pub struct DemoHid {
    info: DeviceInfo,
}

impl Transport for DemoHid {
    fn open(_id: &str) -> Result<Self, String> {
        if clock().0 == Phase::Unplugged {
            return Err("UPS not found".into());
        }
        Ok(Self {
            info: DeviceInfo {
                manufacturer: "Demo".into(),
                product: "Back-UPS DEMO FW:878.L3 .I USB FW:L3".into(),
                serial: "DEMO000000".into(),
                id: "DEMO".into(),
                reader: "demo",
                quirks: Default::default(),
            },
        })
    }

    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn read(&mut self, snap: &mut Snapshot, slow: bool, statics: bool) -> Result<(), String> {
        crate::decode::read_apc_reports(snap, slow, statics, |id| self.feature(id))
    }
}

impl DemoHid {
    fn feature(&mut self, id: u8) -> Result<Report, String> {
        let (phase, t) = clock();
        if phase == Phase::Unplugged {
            return Err("device not connected".into());
        }
        let wobble = (t * 1.7).sin();
        let (ac, chg, dis, charge, runtime, vin, load, bv, lowb): (u8, u8, u8, u8, u16, u16, u8, u16, u8) = match phase
        {
            Phase::Online => (
                1,
                0,
                0,
                100,
                5400,
                (232.0 + 2.0 * wobble) as u16,
                (30.0 + 4.0 * wobble) as u8,
                2756,
                0,
            ),
            Phase::OnBattery => {
                let k = (t - 15.0) / 20.0;
                let c = (100.0 - 85.0 * k) as u8;
                (
                    0,
                    0,
                    1,
                    c,
                    (3600.0 * (1.0 - k)) as u16 + 300,
                    0,
                    34,
                    2500 - (200.0 * k) as u16,
                    0,
                )
            }
            Phase::LowBattery => (0, 0, 1, 9, 110, 0, 34, 2260, 1),
            Phase::Charging => {
                let c = (20.0 + (t - 52.0) * 3.0) as u8;
                (1, 1, 0, c.min(100), 1800, (229.0 + 2.0 * wobble) as u16, 31, 2700, 0)
            }
            Phase::Unplugged => unreachable!(),
        };
        let mut r = [0u8; 8];
        r[0] = id;
        let put16 = |r: &mut Report, v: u16| r[1..3].copy_from_slice(&v.to_le_bytes());
        let r = &mut r;
        match id {
            0x06 => {
                r[1] = chg;
                r[2] = dis;
                r[3] = if ac == 1 { 8 } else { 0 };
            }
            0x0C => {
                r[1] = charge;
                r[2..4].copy_from_slice(&runtime.to_le_bytes());
            }
            0x13 => r[1] = ac,
            0x14 => r[1] = lowb,
            0x09 => put16(r, bv),
            0x08 => put16(r, 2400),
            0x31 => put16(r, vin),
            0x30 => put16(r, 230),
            0x32 => put16(r, 176),
            0x33 => put16(r, 294),
            0x50 => r[1] = load,
            0x52 => put16(r, 865),
            0x35 => r[1] = 1,
            0x36 => r[1] = if ac == 1 && phase == Phase::Charging { 4 } else { 3 },
            0x18 => r[1] = 2,
            0x21 => r[1] = 6,
            0x07 => put16(r, 16205),
            0x20 => put16(r, 21885),
            0x1C => r[1..4].copy_from_slice(&[0x01, 0x25, 0x09]),
            _ => return Err(format!("demo: no report 0x{id:02X}")),
        }
        Ok(*r)
    }
}
