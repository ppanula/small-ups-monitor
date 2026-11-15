//! App / tray icon: a battery outline in the state color, filled to the
//! charge level. Drawn with signed distance fields so it is antialiased at
//! any size (16 px tray up to the 256 px exe icon).

use crate::UpsState;

/// sRGB color for a state (also used by the UI).
pub fn state_rgb(state: UpsState, connected: bool) -> (u8, u8, u8) {
    if !connected {
        return (0x80, 0x86, 0x8E);
    }
    match state {
        UpsState::Online | UpsState::Charging => (0x3D, 0xD6, 0x8C),
        UpsState::OnBattery => (0xFF, 0xB0, 0x20),
        UpsState::LowBattery | UpsState::ShutdownImminent => (0xFF, 0x5A, 0x5F),
        UpsState::Unknown => (0x80, 0x86, 0x8E),
    }
}

/// Signed distance to a rounded rectangle (negative inside).
fn sd_round_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let r = r.min(hw).min(hh);
    let qx = (px - cx).abs() - hw + r;
    let qy = (py - cy).abs() - hh + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

fn coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Straight-alpha RGBA pixels, row-major, top row first.
pub fn tray_rgba(size: u32, state: UpsState, connected: bool, charge: Option<u8>) -> Vec<u8> {
    let (r, g, b) = state_rgb(state, connected);
    let s = size as f32;
    let level = if connected {
        f32::from(charge.unwrap_or(100).min(100)) / 100.0
    } else {
        0.0
    };

    // Horizontal battery: body + terminal nub on the right. Edges sit on whole
    // pixels so small tray sizes stay crisp; only the corners are antialiased.
    let px_round = |v: f32| v.round();
    let stroke = px_round(s * 0.075).max(1.0);
    let gap = px_round(s * 0.05).max(1.0);
    let (bx0, bx1) = (px_round(s * 0.02), px_round(s * 0.85));
    let by0 = px_round(s * 0.2);
    let by1 = s - by0;
    let body_r = px_round(s * 0.12).max(1.0);
    let ny0 = px_round(s * 0.36);
    let (nx0, nx1, ny1) = (bx1 - stroke, s - px_round(s * 0.02), s - ny0);
    let (ix0, iy0) = (bx0 + stroke + gap, by0 + stroke + gap);
    let (ix1, iy1) = (bx1 - stroke - gap, by1 - stroke - gap);
    let fill_x1 = if s <= 32.0 {
        px_round(ix0 + (ix1 - ix0) * level)
    } else {
        ix0 + (ix1 - ix0) * level
    };
    let inner_r = (body_r - stroke - gap).max(0.5);

    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let outer = sd_round_rect(fx, fy, bx0, by0, bx1, by1, body_r);
            let ring = coverage(outer) * (1.0 - coverage(outer + stroke));
            let nub = coverage(sd_round_rect(fx, fy, nx0, ny0, nx1, ny1, (s * 0.04).max(0.5)));
            let fill = if level > 0.0 {
                let inner = sd_round_rect(fx, fy, ix0, iy0, ix1, iy1, inner_r);
                coverage(inner) * coverage(fx - fill_x1)
            } else {
                0.0
            };
            let empty = coverage(outer + stroke) * 0.18; // faint body so it reads on any taskbar
            let a = ring.max(nub).max(fill).max(empty);
            if a <= 0.0 {
                continue;
            }
            let i = ((y * size + x) * 4) as usize;
            px[i] = r;
            px[i + 1] = g;
            px[i + 2] = b;
            px[i + 3] = (255.0 * a).round() as u8;
        }
    }
    px
}
