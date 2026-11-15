# Security policy

## Supported versions

Only the latest release gets fixes. Please update to the newest version from [Releases](https://github.com/ppanula/small-ups-monitor/releases) before reporting.

| Version | Supported |
|---|---|
| latest 1.x | yes |
| older | no |

## Reporting a vulnerability

**Please don't report security problems in public issues.**

Use GitHub's private reporting instead: [**Report a vulnerability**](https://github.com/ppanula/small-ups-monitor/security/advisories/new) (repository → **Security** → **Report a vulnerability**). Only the maintainer can see the report.

Please include:
- the Small UPS Monitor version and Windows version;
- what an attacker could do, and what they need for it (local access, a specially crafted USB device, …);
- steps to reproduce, or a proof of concept.

This is a hobby project maintained in spare time. You can expect an acknowledgement within about a week, and a fix as soon as practical, depending on severity. Once a fix is released, the issue is disclosed in a GitHub security advisory; you'll be credited unless you prefer not to be.

## What the app does (for assessing impact)

- **Reads UPS data only.** It never writes to the UPS: no settings changes and no shutdown commands. It opens USB HID devices through Windows' built-in `hid.dll`, and installs no drivers or services.
- **Runs as the current user.** It needs no administrator rights and doesn't request elevation.
- **Writes only:**
  - `%APPDATA%\Small UPS Monitor\`: settings, the event log and UPS reports;
  - the value `Small UPS Monitor` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, when **Start with Windows** is turned on.
- **No network access of its own.** The only exception is **Report a UPS on GitHub…**, which opens the GitHub issue page in your default browser.
- **Parses data from USB devices.** The report layout and values come from whatever device claims to be a UPS. Bugs in handling unusual or malicious device data are in scope.

## Verifying downloads

Each release includes `small-ups-monitor.exe.sha256`. Check the exe in PowerShell:

```powershell
(Get-FileHash .\small-ups-monitor.exe -Algorithm SHA256).Hash.ToLower()
```

Compare the result with the hash in the `.sha256` file. Release binaries are built by GitHub Actions from the tagged source ([release workflow](.github/workflows/release.yml)). They are not code-signed, so Windows SmartScreen may warn on first run.
