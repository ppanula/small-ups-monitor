# Building for Windows 7 / Windows Server 2008 R2

The normal `cargo build --release` produces an exe that needs Windows 10 or 11.
That's because modern Rust's standard library targets Windows 10, and the app
calls a few Windows 10-only DPI functions.

To build an exe that runs on **Windows 7** and **Windows Server 2008 R2** (both
are NT 6.1), use the `x86_64-win7-windows-msvc` target. The same exe also runs
on Windows 10/11, using per-monitor DPI v2 there.

## One-time setup

```powershell
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

You also need the same MSVC C++ Build Tools as the normal build (see the main
README).

## Build

```powershell
cargo +nightly build -Z build-std --target x86_64-win7-windows-msvc --release
```

The exe is `target\x86_64-win7-windows-msvc\release\small-ups-monitor.exe`.

`-Z build-std` rebuilds the standard library from source for the Win7 baseline
(no prebuilt std is shipped for this Tier 3 target), so the first build is
slower. The C runtime is still linked statically (`crt-static`), so the exe
needs no Visual C++ Redistributable and no Universal C Runtime update.

## Why it works

Three things make the Win7 build possible; all are already set up in this repo:

1. **`windows-sys` is target-gated.** The Windows 7 target uses 0.60, the last
   version that links only Win7-safe DLLs; 0.61+ switched to
   `raw-dylib`/`windows-link` and pulls in `combase.dll` (which does not exist
   on Windows 7). Modern targets keep the latest 0.61. The gating lives in
   `core/Cargo.toml` and `app/Cargo.toml` (`cfg(target_vendor = "win7")`).
   See [microsoft/windows-rs#3808](https://github.com/microsoft/windows-rs/issues/3808).

2. **The DPI code no longer statically imports Windows 10 functions.** The exe
   used to import `SetProcessDpiAwarenessContext`, `GetDpiForWindow`,
   `AdjustWindowRectExForDpi` and `GetSystemMetricsForDpi`, which do not exist
   in Windows 7's `user32.dll` and would stop the exe from even loading. These
   are now loaded with `LoadLibraryW`/`GetProcAddress` and fall back to
   `SetProcessDPIAware` + `GetDeviceCaps` + `AdjustWindowRectEx` +
   `GetSystemMetrics` on Windows 7 (see `app/src/main.rs` and `app/src/tray.rs`).

3. **The target itself** sets the minimum Windows version to 7, so Rust's
   standard library avoids Windows 8/10-only imports such as
   `GetSystemTimePreciseAsFileTime` and `WaitOnAddress`.

## Caveats

- Windows 7 and Server 2008 R2 are out of support; test on a VM or spare
  machine. The app itself has only been smoke-tested, not exhaustively tested,
  on Windows 7.
- On Windows 7 there is no per-monitor DPI: the app is system-DPI aware, so it
  scales to the one system DPI setting and does not rescale when moved between
  monitors.
- The `x86_64-win7-windows-msvc` target is Tier 3 (community-maintained). It
  needs nightly and `-Z build-std`, and can break as Rust changes.
