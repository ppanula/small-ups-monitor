# APC Back-UPS USB HID reference (BR1500GI)

Notes on how an APC Back-UPS talks over USB, collected while building [Small UPS Monitor](../README.md). Everything was verified on one **APC Back-UPS BR1500GI** (firmware `878.L3 .I`) in November 2026, including a real mains outage.

Other Back-UPS models are likely similar but may differ. Marked as *observed* = read from the device; *inferred* = conclusion from observations and public mappings; *unknown* = APC vendor data without a known meaning.

## Contents

1. [Summary](#1-summary)
2. [The device](#2-the-device)
3. [PowerChute Personal Edition](#3-powerchute-personal-edition)
4. [HID structure](#4-hid-structure)
5. [Report map](#5-report-map)
6. [Decoding](#6-decoding)
7. [Status and power events](#7-status-and-power-events)
8. [Not available and caveats](#8-not-available-and-caveats)
9. [Reading it on Windows](#9-reading-it-on-windows)
10. [Appendix: raw snapshot](#appendix-raw-snapshot)
11. [References](#references)

---

## 1. Summary

- **Standard USB HID Power Device.** The UPS is USB ID `051D:0002`, a HID "Power Device" (usage page `0x84`, top-level usage `0x04` UPS). Windows drives it with its built-in HID and HID-battery drivers; no APC driver is involved.
- **Everything is in feature reports.** Every value can be read with `HidD_GetFeature` (HID `GET_REPORT`). A few reports are also input reports.
- **Every report is 5 bytes:** the report ID, then up to 4 little-endian data bytes.
- **Reads are shared.** Another program can read the UPS while PowerChute is running.
- **Main values and their reports:** status `0x06` / `0x13` / `0x14` / `0x16`, charge and runtime `0x0C`, battery voltage `0x09`, input voltage `0x31`, load `0x50`, rated watts `0x52`.
- **On battery:** `ACPresent` goes 1 → 0, `Discharging` 0 → 1, and `APCStatusFlag` 8 → **16**, all within the same second.
- **Not reported:** output voltage, frequencies, temperature and real power. Any wattage is rated W × load %.

## 2. The device

### 2.1 Identity (observed)

| Property | Value |
|---|---|
| Model | Back-UPS BR1500GI (1500 VA / 865 W) |
| HID product string | `Back-UPS BR1500GI FW:878.L3 .I USB FW:L3` |
| Manufacturer string | `American Power Conversion` |
| UPS firmware | `878.L3 .I` |
| USB firmware | `L3` |
| USB vendor / product ID | `0x051D` / `0x0002` |
| USB device release | `0x0090` |
| Serial number | 12 characters (also part of the USB instance ID) |
| Windows device classes | *Human Interface Devices → American Power Conversion USB UPS*, and *Batteries → HID UPS Battery* |
| Windows drivers | `HidUsb`, `HidBatt` (built in) |

### 2.2 HID top-level collection (observed)

```
UsagePage = 0x0084, Usage = 0x0004 (UPS)
InputReportByteLength   = 5
OutputReportByteLength  = 0
FeatureReportByteLength = 5
Input:   9 value caps, 11 flag (button) caps
Feature: 70 value caps, 22 flag (button) caps
Link collections: 11
```

Windows exposes more than one HID interface for this USB device. Select the one whose `HidP_GetCaps` reports usage page `0x84` / usage `0x04`, not the product string. The *HID UPS Battery* child interfaces may return `ERROR_ACCESS_DENIED` without elevation and aren't needed.

## 3. PowerChute Personal Edition

Observed with PowerChute Personal Edition 3.1.0 (`C:\Program Files (x86)\APC\PowerChute Personal Edition\`).

| Component | Role |
|---|---|
| `mainserv.exe` (APC UPS Service) | Runs as LocalSystem and polls the UPS. |
| `UpsDevice.dll` | Its HID layer: imports `HID.DLL` and uses `HidD_GetHidGuid`, `HidD_GetFeature`, etc. |
| `PowerChute.exe`, `apcsystray.exe` | The GUI and tray. They talk to the service over RPC/ALPC; no TCP port is opened. |
| `dataserv.exe` (APC Data Service) | A .NET service that writes energy history to `EnergyLog.mdb`. |

**What this means in practice:**
- **Reading:** PowerChute uses the standard HID API. Reading the UPS directly with `HidD_GetFeature` works alongside it.
- **Writing** (beeper, sensitivity, delays) could conflict with the service. Stop the *APC UPS Service* before experimenting with writes.

**Figures PowerChute computes itself** (cross-checked 2026-11-15):
- **Watts:** "207 Watts" is exactly `865 W × 24 %`, and "4.9824 kWh per day" is exactly `207.6 W × 24 h`. There is no measured-power field; PowerChute uses rated W × load %. Its cost figure is that energy × the tariff set in PowerChute.
- **"Last transfer caused by: Blackout on <date>"** doesn't come from `APCLineFailCause`, which still read `3` after a blackout. PowerChute records it itself.
- **"Result of last manual self-test: Passed on <date>"** comes from PowerChute's own history. The UPS's `Test` value reads `6` (no test initiated).

## 4. HID structure

### 4.1 Usage pages

| Page | Name | Used for |
|---|---|---|
| `0x0084` | HID Power Device | UPS, PowerSummary, Battery, Input, PowerConverter, voltages, load, delays, test, beeper |
| `0x0085` | HID Battery System | charge, runtime, capacity limits, chemistry, `ACPresent`, `Charging`, `Discharging` |
| `0xFF86` | APC vendor-defined | status flag, sensitivity, line-fail cause, battery replace date, firmware, unknown values |

### 4.2 Link collections (observed)

| Index | Usage | Name |
|---:|---|---|
| 0 | `0x84/0x04` | UPS (top level) |
| 1 | `0x84/0x24` | PowerSummary |
| 2 | `0x84/0x02` | PowerSummary.PresentStatus |
| 3 | `0x84/0x12` | Battery |
| 4 | `0x84/0x1A` | Input |
| 5 | `0xFF86/0x05` | APCGeneralCollection |
| 6 | `0x84/0x16` | PowerConverter |
| 7 | `0xFF86/0x01` | APC vendor collection |
| 8 | `0x84/0x02` | PresentStatus (top level) |
| 9 | `0xFF86/0x90` | APC vendor collection |
| 10 | `0xFF86/0x00` | APC vendor collection |

Field paths below are written as `Collection.Usage`, e.g. `UPS.PowerSummary.RemainingCapacity`, the way Windows' preparsed data describes them. Some tools (NUT among them) use slightly different collection names for the same fields.

## 5. Report map

All values were observed on this unit. The *Example* column shows values from 2026-11-14 on mains with about 38 % load. "Data bytes" counts the bytes after the report-ID byte.

### 5.1 Status

| Report | Path | Data bytes | Example | Meaning |
|---:|---|---|---|---|
| `0x06` | `UPS.PowerSummary.Charging` | byte 1 | `0` | 1 = charging |
| `0x06` | `UPS.PowerSummary.Discharging` | byte 2 | `0` | 1 = discharging (on battery) |
| `0x06` | `UPS.PowerSummary.APCStatusFlag` | byte 3 | `8` | 8 = on mains, 16 = on battery (see 7.1) |
| `0x13` | `UPS.PowerSummary.ACPresent` | byte 1 | `1` | 1 = mains present |
| `0x14` | `UPS.PowerSummary.BelowRemainingCapacityLimit` | byte 1 | `0` | 1 = low battery |
| `0x14` | `UPS.PowerSummary.ShutdownImminent` | byte 2 | `0` | 1 = UPS will shut down soon |
| `0x16` | `UPS.PowerSummary.PresentStatus.*` | 11 one-bit flags | | `ACPresent`, `BatteryPresent`, `BelowRemainingCapacityLimit`, `Charging`, `CommunicationLost`, `Discharging`, `NeedReplacement`, `Overload`, `RemainingTimeLimitExpired`, `ShutdownImminent`, `VoltageNotRegulated` |
| `0x7A` | `UPS.PresentStatus.*` | 11 one-bit flags | | same flags at the top level |

Reports `0x06`, `0x0C`, `0x13`, `0x14`, `0x16` and `0x21` are also input reports.

### 5.2 Battery, charge and runtime

| Report | Path | Size | Example | Notes |
|---:|---|---|---|---|
| `0x0C` | `UPS.PowerSummary.RemainingCapacity` | 8 bit, byte 1 | `100` % | |
| `0x0C` | `UPS.PowerSummary.RunTimeToEmpty` | 16 bit, bytes 2–3 | `3792` s | estimate at the current load |
| `0x08` | `UPS.PowerSummary.ConfigVoltage` | 16 bit | `2400` → 24.00 V | nominal battery voltage; exponent 5 |
| `0x09` | `UPS.PowerSummary.Voltage` | 16 bit | `2756` → 27.56 V | battery voltage; exponent 5 |
| `0x22` | `UPS.Battery.RemainingCapacity` | 8 bit | `100` % | same as `0x0C` |
| `0x23` | `UPS.Battery.RunTimeToEmpty` | 16 bit | `3792` s | same as `0x0C` |
| `0x25` | `UPS.Battery.ConfigVoltage` | 16 bit | 24.00 V | same as `0x08` |
| `0x26` | `UPS.Battery.Voltage` | 16 bit | 27.56 V | same as `0x09` |
| `0x0D` | `UPS.PowerSummary.DesignCapacity` | 8 bit | `100` | |
| `0x0E` | `UPS.PowerSummary.FullChargeCapacity` | 8 bit | `100` | |
| `0x0F` | `UPS.PowerSummary.WarningCapacityLimit` | 8 bit | `50` % | |
| `0x11` | `UPS.PowerSummary.RemainingCapacityLimit` | 8 bit | `10` % | low-battery threshold |
| `0x17` / `0x24` | `…RemainingTimeLimit` | 16 bit | `120` s | runtime warning threshold |
| `0x0B` | `UPS.PowerSummary.CapacityMode` | 8 bit | `2` | percent |
| `0x10` / `0x12` | `…CapacityGranularity2` / `1` | 8 bit | `1` / `1` | |
| `0x03` | `UPS.PowerSummary.iDeviceChemistry` | string index | `4` | string "PbAc" (lead-acid) |
| `0x05` | `UPS.PowerSummary.Rechargeable` | 8 bit | `1` | |
| `0x20` | `UPS.Battery.ManufacturerDate` | 16 bit | `21885` → 2022-11-29 | HID date (6.3) |
| `0x1C` | `UPS.Battery.APCBattReplaceDate` | 24 bit | `0x092501` → 2001-09-25 | APC date (6.3); this value looks unset |
| `0x21` | `UPS.Battery.Test` | 8 bit | `6` | self-test result (6.4) |

### 5.3 Input and output

| Report | Path | Size | Example | Notes |
|---:|---|---|---|---|
| `0x31` | `UPS.Input.Voltage` | 16 bit | `232` V | exponent 7 (whole volts) |
| `0x30` | `UPS.Input.ConfigVoltage` | 8 bit | `230` V | nominal input voltage |
| `0x32` | `UPS.Input.LowVoltageTransfer` | 16 bit, logical 156..176 | `176` V | transfers to battery below this |
| `0x33` | `UPS.Input.HighVoltageTransfer` | 16 bit, logical 288..300 | `294` V | transfers to battery above this |
| `0x35` | `UPS.Input.APCSensitivity` | 8 bit, 0..2 | `1` | 0 low, 1 medium, 2 high |
| `0x36` | `UPS.Input.APCLineFailCause` | 8 bit, 0..13 | `3` | see 6.5; not updated live (7.2) |
| `0x50` | `UPS.PowerConverter.PercentLoad` | 8 bit, 0..100 | `38` % | apparent load |
| `0x52` | `UPS.PowerConverter.ConfigActivePower` | 16 bit | `865` W | rated real power, not measured power |

The logical ranges of `0x32` and `0x33` show the transfer voltages the UPS accepts.

### 5.4 Settings and delays

| Report | Path | Size | Example | Notes |
|---:|---|---|---|---|
| `0x18` / `0x78` | `…AudibleAlarmControl` | 8 bit, 1..3 | `2` | beeper: 1 disabled, 2 enabled, 3 muted |
| `0x15` | `UPS.PowerSummary.DelayBeforeShutdown` | 16 bit, signed | `-1` | -1 = no shutdown pending |
| `0x41` | `UPS.APCGeneralCollection.APCDelayBeforeShutdown` | 16 bit, signed | `-1` | |
| `0x42` | `UPS.APCGeneralCollection.DelayBeforeShutdown` | 16 bit, signed | `-1` | |
| `0x40` | `UPS.APCGeneralCollection.APCDelayBeforeReboot` | 8 bit, 0..1 | `0` | |
| `0x79` | `UPS.APCPanelTest` | 8 bit, 0..1 | `0` | front-panel test |

### 5.5 Identity

| Report | Path | Example | Notes |
|---:|---|---|---|
| `0x07` / `0x7B` | `…ManufacturerDate` | `16205` → 2011-10-13 | UPS manufacture date (HID date) |
| `0x7E` | `UPS.APC_UPS_FirmwareRevision` | `7` | string index |
| `0x01`, `0x02`, `0x0A`, `0x7C`, `0x7D`, `0x7F` | `iProduct`, `iSerialNumber`, `iManufacturer` | `2`, `3`, `1`, … | USB string indexes; read the strings with `HidD_GetProductString` etc. |
| `0x04` | `UPS.PowerSummary.iOEMInformation` | `1` | string index |

### 5.6 APC vendor values without a known meaning

Behaviour during the 2026-11-15 outage test is noted where it changed. Don't present these as real measurements without more testing.

| Report | Path | Size, logical range | Example | Observed |
|---:|---|---|---|---|
| `0x27` | `UPS.Battery.ff860024` | 16 bit, 233..254 | `250` | constant |
| `0x34` | `UPS.Input.ff860024` | 16 bit, 117..139 | `127` | constant |
| `0x51` | `UPS.PowerConverter.ff860024` | 8 bit, 190..254 | `243` | constant on battery too, so **not** output voltage |
| `0x28` | `UPS.Battery.ff860018` | 32 bit | | |
| `0x60` | `UPS.ff860001.ff860023` | 16 bit | `0` | |
| `0x61` | `UPS.ff860001.ff860026` | 8 bit, 146..158 | `158` | |
| `0x62` | `UPS.ff860001.ff860025` | 32 bit | `0x03020100` | |
| `0x74` | `UPS.ff86002a` | 32 bit | bytes `CF 29 64 64` | bytes 3–4 fall with charge on battery; byte 4 = charge % |
| `0x75` | `UPS.ff860029` | 16 bit | `98` | 98 on mains, 104 on battery |
| `0x8C` | `UPS.ff860090.ff860091` | 8 bit | `2` | |
| `0x8D` | `UPS.ff860090.ff860092` | 8 bit, 10..80 | `25` | |
| `0x8E` | `UPS.ff860090.ff860093` | 8 bit | ~170 on mains | falls to 0 within 4 s on battery, recovers on mains; tracks charger or mains activity |
| `0x8F` | `UPS.ff860090.ff860094` | 8 bit, 0..3 | `0` | |
| `0x90` | `UPS.ff860090.ff860095` | 8 bit, 0..2 | `1` | |
| `0x91` / `0x92` | `UPS.ff860090.ff860096` / `97` | 16 bit, 0..3600 | `4` / `4` | |
| `0x80` | `UPS.ff860000.ff000055` | 8 bit | `0` | |

## 6. Decoding

### 6.1 Byte layout

Byte 0 of every report is the report ID; data starts at byte 1, little-endian.

- **One value per report** in most reports, starting at byte 1.
- **`0x06`:** three bytes: Charging, Discharging, APCStatusFlag.
- **`0x0C`:** charge (byte 1), then runtime (bytes 2–3).
- **`0x14`:** low battery (byte 1), shutdown imminent (byte 2).
- **`0x16` / `0x7A`:** packed one-bit flags. Read them with `HidP_GetUsages` rather than by bit position.
- **`0x1C`:** 24-bit; `0x28`, `0x62` and `0x74` are 32-bit.
- **Bit sizes and signedness:** use the `BitSize` and `LogicalMin` from `HidP_GetValueCaps`. A negative `LogicalMin` means the value is signed, as in the delays.

### 6.2 Units and scaling

HID units are cgs-based. For volts (`Units = 0x00F0D121`) and watts (`0x0000D121`) that adds a factor of 10⁷:

```
value = raw × 10^(UnitExponent − 7)      volts and watts
value = raw × 10^UnitExponent            seconds (Units 0x1001) and others
```

Examples:
- **Battery voltage:** `2756` with exponent 5 → `2756 × 10⁻²` = **27.56 V**.
- **Input voltage:** `232` with exponent 7 → **232 V**.
- **Rated power:** `865` with exponent 7 → **865 W**.

Percentages are plain integers (0–100), and runtimes are seconds.

### 6.3 Dates

**HID date** (`ManufacturerDate`, usage `0x85/0x85`):

```
year = 1980 + (v >> 9);  month = (v >> 5) & 0x0F;  day = v & 0x1F
16205 → 2011-10-13      21885 → 2022-11-29
```

**APC date** (`APCBattReplaceDate`, `0xFF86/0x16`) is stored as BCD-like hex digits:

```
year  = (v & 0xF) + 10 * ((v >> 4) & 0xF)        // >= 70 → 19xx, else 20xx
day   = ((v >> 8) & 0xF) + 10 * ((v >> 12) & 0xF)
month = ((v >> 16) & 0xF) + 10 * ((v >> 20) & 0xF)
0x092501 → 2001-09-25;  0 = not set
```

### 6.4 Self-test result (`Test`)

| Value | Meaning |
|---:|---|
| 1 | done, passed |
| 2 | done, warning |
| 3 | done, error |
| 4 | aborted |
| 5 | in progress |
| 6 | no test initiated |
| 7 | test scheduled |

### 6.5 Line-fail cause (`APCLineFailCause`)

The values below follow apcupsd's table; NUT documents only some of them.

| Value | Meaning |
|---:|---|
| 0 | no transfers yet |
| 1 | low line voltage |
| 2 | high line voltage |
| 3 | ripple |
| 4 | notch, spike or blackout |
| 5 | self test or discharge calibration |
| 6 | forced (delay before shutdown etc.) |
| 7 | input frequency out of range |
| 8 | notch or blackout |
| 9 | spike or blackout |
| 10 | graceful shutdown by accessories |
| 11 | self test invoked |
| 12 | front-button self test |
| 13 | two-week self test |

## 7. Status and power events

### 7.1 Status values

| Field | Meaning |
|---|---|
| `ACPresent` = 1 / 0 | mains present / on battery |
| `Discharging` = 1 | on battery |
| `Charging` = 1 | charging (after an outage; 0 when full) |
| `APCStatusFlag` = 8 | on mains |
| `APCStatusFlag` = 16 | **on battery on this unit**. NUT treats 16 as a power-on transient. |
| `BelowRemainingCapacityLimit` = 1 | low battery (charge below `RemainingCapacityLimit`, 10 %) |
| `ShutdownImminent` = 1 | the UPS is about to switch off |

For on-battery detection, use `ACPresent` and `Discharging`, not `APCStatusFlag`.

### 7.2 Observed mains outage (2026-11-15)

Raw reports were polled once per second while the UPS was unplugged from the wall for 28 s, at about 26 % load.

| Report | On mains | On battery | Mains back |
|---|---|---|---|
| `0x06` Charging / Discharging / APCStatusFlag | `0 / 0 / 8` | `0 / 1 / 16`, at once | `1 / 0 / 8` (charging) |
| `0x13` ACPresent | `1` | `0`, the same second as `0x06` | `1` |
| `0x31` input voltage | 233–234 V | **0 V** | one `125 V` sample, then 235 V a second before `ACPresent = 1` |
| `0x09` battery voltage | 27.56 V | 27.0 V at once, 25.0 V after about 15 s | 26.0 V, rising |
| `0x0C` charge | 100 % | down to 87 % in 28 s (voltage-based estimate) | stayed at 87 %, then charging |
| `0x0C` runtime | ~1 h 40 min | rose to ~2 h 08 min when the load dropped | recalculated |
| `0x14` low battery / shutdown imminent | `0 / 0` | `0 / 0` | `0 / 0` |
| `0x36` APCLineFailCause | `3` | **unchanged** | unchanged: not updated live |
| `0x51` vendor | `243` | `243` | `243` |
| `0x8E` vendor | ~170 | 0 within 4 s | back towards ~170 |
| `0x74` vendor | `CF 29 64 64` | bytes 3–4 fall with charge | byte 4 = charge |
| `0x75` vendor | `98` | `104` | `98` |

The change appeared in the first poll after the cord was pulled (≤ 1 s).

### 7.3 USB unplug

When the USB cable is pulled, every read fails with Windows error **1617** (`ERROR_DEVICE_REMOVED`). Re-enumerate after the device returns; the old handle doesn't recover. Unplugging USB doesn't affect the UPS's outlets.

## 8. Not available and caveats

- **Not exposed:** output voltage, output frequency, input frequency, temperature and real (measured) power. PowerChute shows `-1` for some of these.
- **Watts are always an estimate:** `ConfigActivePower × PercentLoad / 100`. PowerChute does the same.
- **Charge drops quickly on battery** (100 → 87 % in 28 s at about 26 % load), because the UPS estimates it from voltage. It recovers slowly while charging.
- **`APCBattReplaceDate`** read 2001-09-25, which is older than the UPS itself, so it was probably never set.
- **Writes:** sensitivity, transfer voltages, beeper and delays can be written in principle, but this was not tested. Write one value at a time, with the APC UPS Service stopped. A wrong shutdown-delay write can switch the UPS's outlets off.

## 9. Reading it on Windows

### 9.1 Steps

1. **Enumerate HID interfaces:** `HidD_GetHidGuid`, then SetupAPI (`SetupDiGetClassDevsW`, `SetupDiEnumDeviceInterfaces`, `SetupDiGetDeviceInterfaceDetailW`).
2. **Open each one:** `CreateFileW` with `GENERIC_READ | GENERIC_WRITE` and `FILE_SHARE_READ | FILE_SHARE_WRITE`. If that fails, retry with access `0`; feature reports still work.
3. **Keep only the UPS collection:** `HidD_GetPreparsedData` + `HidP_GetCaps`, with `UsagePage == 0x84 && Usage == 0x04`.
4. **Read a report:**

   ```c
   BYTE buf[5] = { 0x0C };                  // report ID first
   HidD_GetFeature(h, buf, sizeof buf);     // length = FeatureReportByteLength
   int charge  = buf[1];
   int runtime = buf[2] | (buf[3] << 8);    // seconds
   ```

   Alternatively, decode any field with `HidP_GetUsageValue` (values) or `HidP_GetUsages` (flags), using the link collection from its value or button caps.
5. **Poll:** status, charge, runtime, load and voltages every 1–2 s; thresholds and settings every 30 s; dates rarely.

### 9.2 Pitfalls

1. **`HIDP_VALUE_CAPS` is 72 bytes on x64.** A wrong struct definition corrupts memory.
2. **`HIDP_STATUS_SUCCESS` is `0x00110000`, not 0.**
3. **Pass the exact report length** (`FeatureReportByteLength`, 5) to `HidD_GetFeature`.
4. **`HidD_GetInputReport` only works for the input reports** (`0x06`, `0x0C`, `0x13`, `0x14`, `0x16`, `0x21`). Use feature reports for everything else.
5. **The report descriptor isn't readable as a normal user.** The registry copy is access-denied, and `IOCTL_HID_GET_REPORT_DESCRIPTOR` may fail. The preparsed-data API gives everything needed.
6. **Same usage in several collections:** `RemainingCapacity` and `RunTimeToEmpty` appear under both PowerSummary and Battery. Pass the right link collection to `HidP_GetUsageValue`.
7. **Ignore the *HID UPS Battery* child interfaces.**
8. **After an unplug (error 1617), close the handle and enumerate again.**

### 9.3 Tools

- **Small UPS Monitor → right-click tray → Create UPS report…** (or `small-ups-monitor.exe --report`): every field above with its current value, read twice to show which values change. The serial number is hidden. See [Adding your UPS](ADDING-A-UPS.md).
- **`cargo run -p ups-core --features win32 --example rawlog -- 300`:** logs this UPS's raw reports once per second, printing only changes. This is how the outage table in 7.2 was made.
- **`tools/hid_caps.ps1`, `tools/hid_dump.ps1`:** the original read-only PowerShell probes (APC only, value fields only). They print value caps and raw reports, and install nothing.
- **Complete layout of this unit:** [dumps/hiddump-2026-11-15.txt](dumps/hiddump-2026-11-15.txt).

## Appendix: raw snapshot

Captured 2026-11-14 on mains while PowerChute was running. Each line is the 5-byte feature report.

```text
0x06  06 00 00 08 00   Charging=0, Discharging=0, APCStatusFlag=8
0x0C  0C 64 D0 0E 00   RemainingCapacity=100, RunTimeToEmpty=3792 s
0x13  13 01 00 00 00   ACPresent=1
0x14  14 00 00 00 00   BelowRemainingCapacityLimit=0, ShutdownImminent=0
0x21  21 06 00 00 00   Test=6 (no test initiated)
0x07  07 4D 3F 00 00   ManufacturerDate=16205 -> 2011-10-13
0x08  08 60 09 00 00   ConfigVoltage=2400 -> 24.00 V
0x09  09 C4 0A 00 00   Voltage=2756 -> 27.56 V
0x18  18 01 00 00 00   AudibleAlarmControl=1 (disabled)
0x1C  1C 01 25 09 00   APCBattReplaceDate=0x092501 -> 2001-09-25
0x20  20 7D 55 00 00   Battery ManufacturerDate=21885 -> 2022-11-29
0x30  30 E6 00 00 00   Input ConfigVoltage=230 V
0x31  31 E8 00 00 00   Input Voltage=232 V
0x32  32 B0 00 00 00   LowVoltageTransfer=176 V
0x33  33 26 01 00 00   HighVoltageTransfer=294 V
0x35  35 01 00 00 00   APCSensitivity=1 (medium)
0x36  36 03 00 00 00   APCLineFailCause=3 (ripple)
0x50  50 26 00 00 00   PercentLoad=38 %
0x51  51 F3 00 00 00   vendor (unknown)=243
0x52  52 61 03 00 00   ConfigActivePower=865 W
```

## References

- USB HID Power Device Class specification 1.1: <https://www.usb.org/sites/default/files/pdcv11.pdf>
- USB HID Usage Tables: <https://usb.org/document-library/hid-usage-tables-15>
- Microsoft, `HIDP_VALUE_CAPS`: <https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidpi/ns-hidpi-_hidp_value_caps>
- Microsoft, value capability arrays: <https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/value-capability-arrays>
- NUT APC HID mappings (`apc-hid.c`): <https://github.com/networkupstools/nut/blob/master/drivers/apc-hid.c>
