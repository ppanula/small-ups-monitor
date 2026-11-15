# Adding your UPS

Small UPS Monitor reads any UPS that connects to the PC over **USB as a HID device**. That covers most consumer UPSes from APC, Eaton, CyberPower and others. Some models leave values out or report them oddly. A UPS report shows exactly what your UPS sends, so support can be added or fixed. It takes about two minutes.

## 1. Connect the UPS

- Connect the UPS's **USB cable** to the PC. The power cable alone isn't enough.
- Optional check: in **Device Manager**, a HID UPS appears under **Batteries** as "HID UPS Battery". If it appears under **Ports (COM & LPT)** instead, or as an unknown device, it's not a HID UPS. Report it anyway and mention this.

## 2. Run Small UPS Monitor

1. Download `small-ups-monitor.exe` from [Releases](https://github.com/ppanula/small-ups-monitor/releases) and run it. Nothing is installed.
2. Each detected UPS gets its own window and tray icon.
3. If you only see "Please connect UPS", the UPS wasn't detected. Still continue with step 3; the report helps find out why.

## 3. Create a UPS report

**Right-click** a Small UPS Monitor tray icon → **Create UPS report…**

After a few seconds the report opens in Notepad. It's also saved in `%APPDATA%\Small UPS Monitor\reports\`.

You can also run it without the tray:

```
small-ups-monitor.exe --report
```

**What's in the report:**
- the UPS's USB ID, manufacturer and model name;
- what Small UPS Monitor decodes;
- every value the UPS exposes, read twice a few seconds apart, so values that never change are easy to spot.

**What's not in it:** your UPS's **serial number is hidden**, and the report contains no file paths, user names or other information about your PC, apart from the Windows version.

## 4. Compare with the UPS itself

Look at the UPS's display, or its vendor software (PowerChute, PowerPanel, UPS Companion…), and note:
- **load** (% or watts),
- **battery charge**,
- **runtime**,
- **input voltage**.

These show which values Small UPS Monitor gets right and which it doesn't.

## 5. Optional: on-battery test

This is the most useful extra information, because some UPSes only report certain values while on battery.

1. Make sure nothing important depends on the UPS. A lamp or a PC that can lose power is fine. Don't do this with an old battery that might not hold.
2. **Unplug the UPS's power cord** from the wall. The UPS beeps and runs on battery.
3. Check that Small UPS Monitor shows **ON BATTERY**, then right-click the tray icon → **Create UPS report…** again.
4. **Plug the power cord back in** within about 20–30 seconds.

## 6. Send it

1. Right-click the tray icon → **Report a UPS on GitHub…**, or open [New UPS report](https://github.com/ppanula/small-ups-monitor/issues/new?template=ups-report.yml). You need a free GitHub account.
2. Fill in the brand and model and what the UPS shows.
3. Paste the report, or drag the `.txt` file into the form. Add the on-battery report too if you made one.

Thank you! Reports like this are how new UPSes get supported and quirks get fixed.

## For developers

The same report is available from the source tree:

```
cargo run --release -p ups-core --features win32 --example hiddump
```

**Reading a report:**
- **Decoded values** shows what each reader produces. For APC units it shows both the APC reader and the generic reader.
- **Generic reader mapping** shows which HID usage path each field came from.
- **Report layout and values** lists every value and flag the device's descriptor defines, with two raw samples:
  - `*` marks values that changed between the samples.
  - The `exp` column is the HID unit exponent. Volts and watts are scaled by an extra 10⁻⁷, because HID uses cgs units.

To support a new model:
- **Mapping:** add usage-path candidates in `core/src/generic.rs` (`Target::candidates`).
- **Values a UPS reports but doesn't really measure:** add an entry to `quirks()`.
- **A vendor-specific reader:** add one next to the APC reader in `core/src/hid_win32.rs` (`UpsHid`).
