# Changelog

## [1.0.2] - 2026-11-17

- **Windows 7 / Server 2008 R2 support:** a second exe built with the
  `x86_64-win7-windows-msvc` target now runs on Windows 7 and Windows Server
  2008 R2 (as well as Windows 10/11), with no Visual C++ Redistributable or
  other runtime. The normal build is unchanged and still targets Windows 10/11.

## [1.0.1] - 2026-11-15

- **Version is now visible in the app:**
  - at the top of the tray menu; click it to open Releases and check for updates;
  - in **About Small UPS Monitor…** in the window's title-bar menu (click the window icon or press Alt+Space), with the license and project link.
- **Repository:** added SECURITY.md (private vulnerability reporting), .gitattributes and .editorconfig.

## [1.0.0] - 2026-11-15

First public release.

- Monitors USB HID UPSes on Windows 10/11 from a single ~0.5 MB exe: no installer, service, driver or runtime.
- **APC reader** for APC units, tested on a Back-UPS BR1500GI. It also reads APC-specific data: sensitivity, status flag and battery dates.
- **Generic HID reader** for other HID Power Device UPSes, tested on an Eaton 3S 700. It includes per-model quirks for fields a UPS doesn't really measure.
- **One window and tray icon per UPS.**
  - A UPS window closes 10 s after its USB is unplugged, and pops up when the UPS is plugged in.
  - "Please connect UPS" shows when no UPS is connected.
- **Shows:** status, input voltage and transfer range, load with estimated W/VA, charge, runtime, battery voltage, 10-minute charts with gaps for missing data, energy rate, and the last outage with its duration.
- **Notifications** for power failure, power restored, low battery, shutdown imminent, battery needing replacement, and disconnects.
- **Data:** a CSV event log, window positions per UPS, and Start with Windows.
- **UPS reports:** **Create UPS report…** (tray menu) or `--report` writes a privacy-safe report of what the UPS sends; **Report a UPS on GitHub…** opens the issue form.
- **Tools:** `--demo` mode, plus the `hiddump`, `watch` and `rawlog` diagnostic examples.
