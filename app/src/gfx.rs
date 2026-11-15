//! Thin GDI helpers. All coordinates are logical (96-DPI) pixels.

use windows_sys::Win32::Foundation::{POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;

use crate::wide;

pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

pub const BG: u32 = rgb(0x14, 0x16, 0x1B);
pub const CARD: u32 = rgb(0x1E, 0x21, 0x28);
pub const TRACK: u32 = rgb(0x2C, 0x30, 0x39);
pub const TEXT: u32 = rgb(0xE8, 0xEA, 0xED);
pub const DIM: u32 = rgb(0x8B, 0x92, 0x9C);
pub const BLUE: u32 = rgb(0x4D, 0xA3, 0xFF);
pub const GREEN: u32 = rgb(0x3D, 0xD6, 0x8C);
pub const AMBER: u32 = rgb(0xFF, 0xB0, 0x20);
pub const RED: u32 = rgb(0xFF, 0x5A, 0x5F);

pub fn from_tuple((r, g, b): (u8, u8, u8)) -> u32 {
    rgb(r, g, b)
}

/// Mix `c` over `base` with weight `t` (0..1).
pub fn mix(base: u32, c: u32, t: f32) -> u32 {
    let ch = |v: u32, s: u32| ((v >> s) & 0xFF) as f32;
    let m = |s| (ch(base, s) * (1.0 - t) + ch(c, s) * t).round() as u32;
    m(0) | m(8) << 8 | m(16) << 16
}

pub struct Fonts {
    pub title: HFONT,
    pub big: HFONT,
    pub unit: HFONT,
    pub label: HFONT,
    pub small: HFONT,
    pub small_bold: HFONT,
}

fn font(px: i32, weight: i32, face: &str, dpi: u32) -> HFONT {
    let face = wide(face);
    unsafe {
        CreateFontW(
            -(px * dpi as i32 / 96),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            OUT_DEFAULT_PRECIS.into(),
            CLIP_DEFAULT_PRECIS.into(),
            CLEARTYPE_QUALITY.into(),
            0,
            face.as_ptr(),
        )
    }
}

impl Fonts {
    pub fn new(dpi: u32) -> Self {
        let display = "Segoe UI Variable Display";
        let text = "Segoe UI Variable Text";
        Self {
            title: font(17, 600, display, dpi),
            big: font(38, 600, display, dpi),
            unit: font(17, 400, display, dpi),
            label: font(11, 700, text, dpi),
            small: font(12, 400, text, dpi),
            small_bold: font(12, 700, text, dpi),
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in [self.title, self.big, self.unit, self.label, self.small, self.small_bold] {
            unsafe { DeleteObject(f) };
        }
    }
}

pub struct Canvas {
    pub dc: HDC,
    pub dpi: u32,
}

pub const LEFT: u32 = DT_LEFT;
pub const RIGHT: u32 = DT_RIGHT;
pub const CENTER: u32 = DT_CENTER;
pub const BOTTOM: u32 = DT_BOTTOM;

impl Canvas {
    pub fn s(&self, v: i32) -> i32 {
        v * self.dpi as i32 / 96
    }

    pub fn sf(&self, v: f32) -> i32 {
        (v * self.dpi as f32 / 96.0).round() as i32
    }

    fn rect(&self, x: i32, y: i32, w: i32, h: i32) -> RECT {
        RECT {
            left: self.s(x),
            top: self.s(y),
            right: self.s(x + w),
            bottom: self.s(y + h),
        }
    }

    pub fn fill(&self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let r = self.rect(x, y, w, h);
        unsafe {
            SetDCBrushColor(self.dc, color);
            FillRect(self.dc, &r, GetStockObject(DC_BRUSH));
        }
    }

    pub fn round(&self, x: i32, y: i32, w: i32, h: i32, radius: i32, color: u32) {
        let r = self.rect(x, y, w, h);
        let d = self.s(radius * 2);
        unsafe {
            let ob = SelectObject(self.dc, GetStockObject(DC_BRUSH));
            let op = SelectObject(self.dc, GetStockObject(DC_PEN));
            SetDCBrushColor(self.dc, color);
            SetDCPenColor(self.dc, color);
            RoundRect(self.dc, r.left, r.top, r.right + 1, r.bottom + 1, d, d);
            SelectObject(self.dc, ob);
            SelectObject(self.dc, op);
        }
    }

    pub fn circle(&self, cx: f32, cy: f32, radius: f32, color: u32) {
        unsafe {
            let ob = SelectObject(self.dc, GetStockObject(DC_BRUSH));
            let op = SelectObject(self.dc, GetStockObject(DC_PEN));
            SetDCBrushColor(self.dc, color);
            SetDCPenColor(self.dc, color);
            Ellipse(
                self.dc,
                self.sf(cx - radius),
                self.sf(cy - radius),
                self.sf(cx + radius) + 1,
                self.sf(cy + radius) + 1,
            );
            SelectObject(self.dc, ob);
            SelectObject(self.dc, op);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(&self, x: i32, y: i32, w: i32, h: i32, s: &str, font: HFONT, color: u32, align: u32) {
        // DrawTextW dereferences the pointer even for zero length, and an empty
        // Vec's pointer is dangling.
        if s.is_empty() {
            return;
        }
        let mut r = self.rect(x, y, w, h);
        let ws: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            SelectObject(self.dc, font);
            SetTextColor(self.dc, color);
            SetBkMode(self.dc, TRANSPARENT as _);
            let v = if align & DT_BOTTOM != 0 { 0 } else { DT_VCENTER };
            DrawTextW(
                self.dc,
                ws.as_ptr(),
                ws.len() as i32,
                &mut r,
                align | v | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }

    /// Text width in logical pixels.
    pub fn text_w(&self, s: &str, font: HFONT) -> i32 {
        if s.is_empty() {
            return 0;
        }
        let ws: Vec<u16> = s.encode_utf16().collect();
        let mut sz = SIZE { cx: 0, cy: 0 };
        unsafe {
            SelectObject(self.dc, font);
            GetTextExtentPoint32W(self.dc, ws.as_ptr(), ws.len() as i32, &mut sz);
        }
        sz.cx * 96 / self.dpi as i32
    }

    /// Polyline through logical-pixel points.
    pub fn polyline(&self, pts: &[(f32, f32)], width: f32, color: u32) {
        if pts.len() < 2 {
            return;
        }
        let dev: Vec<POINT> = pts
            .iter()
            .map(|&(x, y)| POINT {
                x: self.sf(x),
                y: self.sf(y),
            })
            .collect();
        unsafe {
            let pen = CreatePen(PS_SOLID, self.sf(width).max(1), color);
            let op = SelectObject(self.dc, pen);
            Polyline(self.dc, dev.as_ptr(), dev.len() as i32);
            SelectObject(self.dc, op);
            DeleteObject(pen);
        }
    }

    pub fn hline(&self, x: i32, y: i32, w: i32, color: u32) {
        self.fill(x, y, w, 1, color);
    }
}
