//! Layout and painting (double-buffered GDI).

use std::mem::zeroed;

use ups_core::decode::{beeper_str, sensitivity_str};
use ups_core::history::segments;
use ups_core::icon::state_rgb;
use ups_core::{History, fmt_runtime};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

use crate::gfx::*;
use crate::{App, CLIENT_W, PLACEHOLDER_H, fmt_duration};

const PAD: i32 = 16;
const GAP: i32 = 12;

pub fn paint(a: &mut App) {
    unsafe {
        let mut ps: PAINTSTRUCT = zeroed();
        let hdc = BeginPaint(a.hwnd, &mut ps);
        let mut rc = zeroed();
        GetClientRect(a.hwnd, &mut rc);
        let (w, h) = (rc.right.max(1), rc.bottom.max(1));
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, bmp);

        let c = Canvas { dc: mem, dpi: a.dpi };
        c.fill(0, 0, w * 96 / a.dpi as i32 + 1, h * 96 / a.dpi as i32 + 1, BG);
        draw(&c, a);

        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        EndPaint(a.hwnd, &ps);
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "—".to_string(), |v| v.to_string())
}

fn level_color(pct: u8) -> u32 {
    match pct {
        0..=69 => GREEN,
        70..=89 => AMBER,
        _ => RED,
    }
}

fn card(c: &Canvas, x: i32, y: i32, w: i32, h: i32, label: &str, f: &Fonts) {
    c.round(x, y, w, h, 10, CARD);
    c.text(x + 14, y + 10, w - 28, 16, label, f.label, DIM, LEFT);
}

/// Big number followed by a smaller unit, bottom-aligned.
fn big_value(c: &Canvas, x: i32, y: i32, value: &str, unit: &str, f: &Fonts, color: u32) {
    let vw = c.text_w(value, f.big);
    c.text(x, y, vw + 4, 46, value, f.big, color, LEFT | BOTTOM);
    c.text(x + vw + 4, y, 80, 41, unit, f.unit, DIM, LEFT | BOTTOM);
}

fn bar(c: &Canvas, x: i32, y: i32, w: i32, pct: Option<u8>, color: u32) {
    c.round(x, y, w, 6, 3, TRACK);
    if let Some(p) = pct {
        let fw = w * i32::from(p.min(100)) / 100;
        if fw > 0 {
            c.round(x, y, fw.max(6), 6, 3, color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sparkline(c: &Canvas, x: i32, y: i32, w: i32, h: i32, hist: &History, lo: f32, hi: f32, color: u32) {
    // Faint grid: top, middle, bottom.
    for i in 0..3 {
        c.hline(x, y + i * h / 2, w, mix(CARD, DIM, 0.12));
    }
    let pts: Vec<(f32, f32)> = hist
        .normalized(lo, hi)
        .into_iter()
        .map(|(nx, ny)| (x as f32 + nx * w as f32, (y + h) as f32 - ny * h as f32))
        .collect();
    // Gaps (no data) break the line; a lone sample between gaps is a dot.
    for seg in segments(&pts) {
        if let [(px, py)] = seg {
            c.circle(*px, *py, 1.5, color);
        } else {
            c.polyline(seg, 2.0, color);
        }
    }
    if let Some(&(lx, ly)) = pts.last().filter(|p| p.1.is_finite()) {
        c.circle(lx, ly, 3.0, color);
    }
}

fn draw(c: &Canvas, a: &App) {
    let f = &a.fonts;
    if a.device_id.is_empty() {
        // No UPS connected yet: this window only asks for one.
        c.text(
            PAD,
            0,
            CLIENT_W - 2 * PAD,
            PLACEHOLDER_H,
            "Please connect UPS",
            f.title,
            TEXT,
            CENTER,
        );
        return;
    }
    let s = a.snap.as_ref();
    let connected = a.connected();
    let state = a.state();
    let accent = from_tuple(state_rgb(state, connected));
    let quirks = a.info.as_ref().map(|i| i.quirks).unwrap_or_default();
    // Last known values stay visible but dimmed while there is no live data.
    let val = if connected { TEXT } else { DIM };
    let w = CLIENT_W - 2 * PAD;
    let half = (w - GAP) / 2;
    let x1 = PAD;
    let x2 = PAD + half + GAP;

    // Header: model + status pill.
    let model = a.info.as_ref().map_or("APC UPS", |i| i.model());
    c.text(PAD, 14, w - 130, 24, model, f.title, TEXT, LEFT);
    let sub = match &a.info {
        Some(i) => {
            let mut parts = Vec::new();
            if let Some(fw) = i.firmware() {
                parts.push(format!("FW {fw}"));
            }
            if let Some(sn) = i.real_serial() {
                parts.push(format!("SN {sn}"));
            }
            parts.push(i.reader.to_string());
            parts.join("  ·  ")
        }
        None if a.device_id.is_empty() => "Searching for UPS…".to_string(),
        None => "Connecting…".to_string(),
    };
    c.text(PAD, 38, w, 18, &sub, f.small, DIM, LEFT);
    let label = if connected { state.label() } else { "OFFLINE" };
    let pw = c.text_w(label, f.small_bold) + 28;
    let px = PAD + w - pw;
    c.round(px, 16, pw, 24, 12, mix(BG, accent, 0.22));
    c.circle((px + 13) as f32, 28.0, 3.5, accent);
    c.text(px + 20, 16, pw - 24, 24, label, f.small_bold, accent, LEFT);

    // Row 1: input voltage, load.
    let y = 70;
    let ch = 112;
    card(c, x1, y, half, ch, "INPUT VOLTAGE", f);
    big_value(c, x1 + 14, y + 26, &opt(s.and_then(|s| s.input_v)), "V", f, val);
    // Position of the input voltage between the transfer thresholds.
    let (lo, hi) = (s.and_then(|s| s.transfer_low), s.and_then(|s| s.transfer_high));
    c.round(x1 + 14, y + 78, half - 28, 6, 3, TRACK);
    if let (Some(lo), Some(hi), Some(v)) = (lo, hi, s.and_then(|s| s.input_v)) {
        let t = (f32::from(v.clamp(lo, hi)) - f32::from(lo)) / f32::from((hi - lo).max(1));
        let mx = (x1 + 14) as f32 + t * (half - 28) as f32;
        c.circle(mx, (y + 81) as f32, 5.0, if connected { BLUE } else { DIM });
    }
    let range = match (lo, hi) {
        (Some(lo), Some(hi)) => format!("{lo}  ·  nominal {}  ·  {hi}", opt(s.and_then(|s| s.input_v_nominal))),
        _ => "transfer range —".into(),
    };
    c.text(x1 + 14, y + 88, half - 28, 16, &range, f.small, DIM, CENTER);

    card(c, x2, y, half, ch, "LOAD", f);
    let load = s.and_then(|s| s.load_pct);
    big_value(c, x2 + 14, y + 26, &opt(load), "%", f, val);
    bar(
        c,
        x2 + 14,
        y + 78,
        half - 28,
        load,
        if connected { level_color(load.unwrap_or(0)) } else { DIM },
    );
    // Watts when the UPS has a watt rating, else VA (e.g. Eaton reports only VA).
    let watts = match (
        s.and_then(|s| s.est_watts()),
        s.and_then(|s| s.rated_w),
        s.and_then(|s| s.est_va()),
        s.and_then(|s| s.rated_va),
    ) {
        (Some(e), Some(r), _, _) => format!("≈ {e} W of {r} W (est.)"),
        (_, _, Some(e), Some(r)) => format!("≈ {e} VA of {r} VA (est.)"),
        _ if quirks.load_not_measured => "not measured by this UPS".into(),
        _ => "—".into(),
    };
    c.text(x2 + 14, y + 88, half - 28, 16, &watts, f.small, DIM, LEFT);

    // Row 2: battery, runtime.
    let y = y + ch + GAP;
    card(c, x1, y, half, ch, "BATTERY", f);
    let charge = s.and_then(|s| s.charge);
    big_value(c, x1 + 14, y + 26, &opt(charge), "%", f, val);
    bar(
        c,
        x1 + 14,
        y + 78,
        half - 28,
        charge,
        if connected { accent } else { DIM },
    );
    let bv = match (s.and_then(|s| s.battery_v), s.and_then(|s| s.battery_v_nominal)) {
        (Some(v), Some(n)) => format!("{v:.1} V  ·  nominal {n:.0} V"),
        (Some(v), None) => format!("{v:.1} V"),
        _ if s.is_some() => "voltage not reported".into(),
        _ => "—".into(),
    };
    c.text(x1 + 14, y + 88, half - 28, 16, &bv, f.small, DIM, LEFT);

    card(c, x2, y, half, ch, "RUNTIME", f);
    let (rv, ru) = match s.and_then(|s| s.runtime_s) {
        Some(sec) if sec >= 3600 => (format!("{}:{:02}", sec / 3600, sec / 60 % 60), "h"),
        Some(sec) => ((sec / 60).to_string(), "min"),
        None => ("—".into(), ""),
    };
    big_value(c, x2 + 14, y + 26, &rv, ru, f, val);
    let rnote = match (quirks.runtime_from_charge, state.on_battery()) {
        (true, _) => "rough: from charge only",
        (false, true) => "remaining on battery",
        (false, false) => "estimated at current load",
    };
    c.text(x2 + 14, y + 80, half - 28, 16, rnote, f.small, DIM, LEFT);
    let status = s.map(|s| s.status_tokens()).unwrap_or_default();
    c.text(
        x2 + 14,
        y + 92,
        half - 28,
        16,
        &format!("status {status}"),
        f.small,
        DIM,
        LEFT,
    );

    // Row 3 + 4: sparklines.
    let y = y + ch + GAP;
    let sh = 84;
    card(c, PAD, y, w, sh, "INPUT VOLTAGE  ·  10 MIN", f);
    if s.is_some_and(|s| s.input_v.is_none()) && a.hist_v.min_max().is_none() {
        c.text(
            PAD + 14,
            y + 32,
            w - 28,
            sh - 44,
            "not reported by this UPS",
            f.small,
            DIM,
            CENTER,
        );
    } else if let Some((mn, mx)) = a.hist_v.min_max() {
        c.text(
            PAD + 14,
            y + 10,
            w - 28,
            16,
            &format!("min {mn:.0}  ·  max {mx:.0} V"),
            f.small,
            DIM,
            RIGHT,
        );
        let mid = (mn + mx) / 2.0;
        let span = (mx - mn).max(6.0) / 2.0 + 1.0;
        sparkline(
            c,
            PAD + 14,
            y + 32,
            w - 28,
            sh - 44,
            &a.hist_v,
            mid - span,
            mid + span,
            BLUE,
        );
    }

    let y = y + sh + GAP;
    card(c, PAD, y, w, sh, "LOAD  ·  10 MIN", f);
    if quirks.load_not_measured {
        c.text(
            PAD + 14,
            y + 32,
            w - 28,
            sh - 44,
            "not measured by this UPS",
            f.small,
            DIM,
            CENTER,
        );
    } else if let (Some((_, mx)), Some(avg)) = (a.hist_load.min_max(), a.hist_load.mean()) {
        // Energy rate from the 10-minute average load, the same estimate PowerChute uses.
        let energy = match s.and_then(|s| s.rated_w) {
            Some(r) => format!(
                "avg {avg:.0} %  ·  ≈ {:.1} kWh/day",
                f32::from(r) * avg / 100.0 * 24.0 / 1000.0
            ),
            None => format!("avg {avg:.0} %"),
        };
        c.text(PAD + 14, y + 10, w - 28, 16, &energy, f.small, DIM, RIGHT);
        let top = (mx + 10.0).clamp(20.0, 100.0);
        sparkline(
            c,
            PAD + 14,
            y + 32,
            w - 28,
            sh - 44,
            &a.hist_load,
            0.0,
            top,
            level_color(mx as u8),
        );
    }

    // Details.
    let y = y + sh + GAP;
    let dh = 82;
    c.round(PAD, y, w, dh, 10, CARD);
    let rows = [
        // Sensitivity is APC-only; other UPSes show their rating there instead.
        match (
            s.and_then(|s| s.sensitivity),
            s.and_then(|s| s.rated_w),
            s.and_then(|s| s.rated_va),
        ) {
            (Some(v), _, _) => (
                "Sensitivity",
                sensitivity_str(v).to_string(),
                "Beeper",
                opt(s.and_then(|s| s.beeper).map(beeper_str)),
            ),
            (None, Some(w), _) => (
                "Rating",
                format!("{w} W"),
                "Beeper",
                opt(s.and_then(|s| s.beeper).map(beeper_str)),
            ),
            (None, None, Some(va)) => (
                "Rating",
                format!("{va} VA"),
                "Beeper",
                opt(s.and_then(|s| s.beeper).map(beeper_str)),
            ),
            _ => (
                "Sensitivity",
                "—".to_string(),
                "Beeper",
                opt(s.and_then(|s| s.beeper).map(beeper_str)),
            ),
        },
        (
            "Battery date",
            opt(s.and_then(|s| s.battery_mfr_date)),
            "UPS made",
            opt(s.and_then(|s| s.mfr_date)),
        ),
    ];
    for (i, (k1, v1, k2, v2)) in rows.iter().enumerate() {
        let ry = y + 10 + if i == 0 { 0 } else { 42 };
        c.text(PAD + 14, ry, 90, 20, k1, f.small, DIM, LEFT);
        c.text(PAD + 100, ry, half - 90, 20, v1, f.small, TEXT, LEFT);
        c.text(x2 + 6, ry, 80, 20, k2, f.small, DIM, LEFT);
        c.text(x2 + 80, ry, half - 86, 20, v2, f.small, TEXT, LEFT);
    }
    // Last outage spans the full width (the UPS's own "last transfer cause" is not kept up to date).
    let outage = match (&a.last_outage, a.outage_start) {
        (_, Some(t)) if state.on_battery() => format!("now  ·  on battery for {}", fmt_duration(t.elapsed().as_secs())),
        (Some(o), _) => {
            let when = o.start.get(..16).unwrap_or(&o.start);
            match o.secs {
                Some(secs) => format!("{when}  ·  {} on battery", fmt_duration(secs)),
                None => when.to_string(),
            }
        }
        (None, _) => "none recorded".into(),
    };
    c.text(PAD + 14, y + 31, 90, 20, "Last outage", f.small, DIM, LEFT);
    c.text(PAD + 100, y + 31, w - 114, 20, &outage, f.small, TEXT, LEFT);

    // Footer.
    let y = y + dh + 8;
    let (msg, col) = if let Some(e) = &a.error {
        match a.last_data {
            Some(t) => (
                format!("{e}  ·  last data {} ago", fmt_duration(t.elapsed().as_secs())),
                RED,
            ),
            None => (e.clone(), RED),
        }
    } else if let Some(t) = a.last_data {
        let age = t.elapsed().as_secs();
        if connected {
            (
                format!(
                    "Updated {age} s ago  ·  {}",
                    fmt_runtime(s.and_then(|s| s.runtime_s).unwrap_or(0))
                ),
                DIM,
            )
        } else {
            (format!("No data for {age} s"), AMBER)
        }
    } else {
        ("Connecting…".into(), DIM)
    };
    c.text(PAD, y, w, 18, &msg, f.small, col, CENTER);
}
