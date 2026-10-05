//! Drawing for the strip and its flyouts. These are per-pixel-alpha layered
//! windows, which is what allows a translucent background with solid icons and
//! text, anti-aliased rounded corners, and real transparency outside the shape.
//!
//! Shapes are drawn with tiny-skia. Text is drawn by GDI into a grey coverage
//! mask (GDI can't write alpha) and then blended in the requested colour.

use super::icons::{self, Source as IconSource};
use super::ui::wide;
use crate::appearance::Rgba;
use resvg::tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_CALCRECT, DT_NOPREFIX,
    DT_SINGLELINE, DeleteDC, DeleteObject, DrawTextW, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
    FONT_QUALITY, FW_NORMAL, FW_SEMIBOLD, GdiFlush, GetDC, GetDIBits, HBITMAP, HDC, HFONT, HGDIOBJ, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::PCWSTR;

pub fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(c));
    p.anti_alias = true;
    p
}

pub fn round_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<resvg::tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?));
    }
    // Cubic approximation of a quarter circle.
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub struct Canvas {
    pub pix: Pixmap,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Canvas> {
        Some(Canvas { pix: Pixmap::new(w.max(1) as u32, h.max(1) as u32)? })
    }

    pub fn width(&self) -> i32 {
        self.pix.width() as i32
    }

    pub fn height(&self) -> i32 {
        self.pix.height() as i32
    }

    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        if c.a == 0 {
            return;
        }
        if let Some(path) = round_rect_path(x, y, w, h, r) {
            self.pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
        }
    }

    /// An outline drawn inside the given box.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, c: Rgba) {
        if c.a == 0 || width <= 0.0 {
            return;
        }
        let half = width / 2.0;
        if let Some(path) = round_rect_path(x + half, y + half, w - width, h - width, (r - half).max(0.0)) {
            let stroke = Stroke { width, ..Default::default() };
            self.pix.stroke_path(&path, &paint(c), &stroke, Transform::identity(), None);
        }
    }

    pub fn image(&mut self, img: &Pixmap, x: i32, y: i32, size: i32, alpha: f32) {
        let sx = size as f32 / img.width() as f32;
        let sy = size as f32 / img.height() as f32;
        let paint =
            PixmapPaint { opacity: alpha, quality: resvg::tiny_skia::FilterQuality::Bicubic, ..Default::default() };
        let t = Transform::from_row(sx, 0.0, 0.0, sy, x as f32, y as f32);
        self.pix.draw_pixmap(0, 0, img.as_ref(), &paint, t, None);
    }

    /// Small downward-pointing triangle (the "▾" marking a category).
    pub fn chevron(&mut self, cx: f32, cy: f32, size: f32, c: Rgba) {
        let mut pb = PathBuilder::new();
        pb.move_to(cx - size / 2.0, cy - size / 4.0);
        pb.line_to(cx + size / 2.0, cy - size / 4.0);
        pb.line_to(cx, cy + size / 4.0);
        pb.close();
        if let Some(path) = pb.finish() {
            self.pix.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
        }
    }

    /// Settings cog.
    pub fn gear(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let p = paint(c);
        for i in 0..8 {
            let angle = i as f32 * 45.0;
            if let Some(rect) = Rect::from_xywh(-r * 0.2, -r, r * 0.4, r * 0.5) {
                let path = PathBuilder::from_rect(rect);
                let t = Transform::from_rotate(angle).post_translate(cx, cy);
                self.pix.fill_path(&path, &p, FillRule::Winding, t, None);
            }
        }
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r * 0.72);
        pb.push_circle(cx, cy, r * 0.3);
        if let Some(path) = pb.finish() {
            self.pix.fill_path(&path, &p, FillRule::EvenOdd, Transform::identity(), None);
        }
    }

    /// Chain-link glyph.
    pub fn link(&mut self, cx: f32, cy: f32, size: f32, c: Rgba) {
        let (w, h) = (size * 0.62, size * 0.36);
        let stroke = Stroke { width: (size * 0.11).max(1.2), ..Default::default() };
        for dx in [-size * 0.17, size * 0.17] {
            if let Some(path) = round_rect_path(dx - w / 2.0, -h / 2.0, w, h, h / 2.0) {
                let t = Transform::from_rotate(-45.0).post_translate(cx, cy);
                self.pix.stroke_path(&path, &paint(c), &stroke, t, None);
            }
        }
    }

    /// Simple folder glyph, used when a category has no icon of its own.
    pub fn folder(&mut self, x: f32, y: f32, size: f32, c: Rgba) {
        self.fill_round_rect(x, y + size * 0.12, size * 0.45, size * 0.2, size * 0.06, c);
        self.fill_round_rect(x, y + size * 0.22, size, size * 0.66, size * 0.08, c);
    }

    /// Draws text in `rect` (DT_* layout flags) in colour `c`.
    pub fn text(&mut self, text: &str, rect: RECT, font: HFONT, c: Rgba, flags: DRAW_TEXT_FORMAT) {
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 || text.is_empty() || c.a == 0 {
            return;
        }
        let Some(mask) = text_mask(text, w, h, font, flags) else { return };
        // Blend colour × coverage over the existing premultiplied pixels.
        let (cw, ch) = (self.width(), self.height());
        let data = self.pix.data_mut();
        for row in 0..h {
            let py = rect.top + row;
            if py < 0 || py >= ch {
                continue;
            }
            for col in 0..w {
                let px = rect.left + col;
                if px < 0 || px >= cw {
                    continue;
                }
                let cov = mask[(row * w + col) as usize] as u32;
                if cov == 0 {
                    continue;
                }
                let a = cov * c.a as u32 / 255;
                let i = ((py * cw + px) * 4) as usize;
                let inv = 255 - a;
                data[i] = ((c.r as u32 * a + data[i] as u32 * inv) / 255) as u8;
                data[i + 1] = ((c.g as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
                data[i + 2] = ((c.b as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
                data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
            }
        }
    }

    /// Puts the frame on screen at (`x`, `y`).
    pub fn present(&self, hwnd: HWND, x: i32, y: i32) {
        let (w, h) = (self.width(), self.height());
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(mem), &bitmap_info(w, h), DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(mem);
                ReleaseDC(None, screen);
                return;
            };
            // tiny-skia is premultiplied RGBA; GDI wants premultiplied BGRA.
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(self.pix.data().as_chunks::<4>().0) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
            let old = SelectObject(mem, HGDIOBJ(bmp.0));
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&POINT { x, y }),
                Some(&SIZE { cx: w, cy: h }),
                Some(mem),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
        }
    }
}

fn bitmap_info(w: i32, h: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Renders white-on-black text with GDI and returns per-pixel coverage.
fn text_mask(text: &str, w: i32, h: i32, font: HFONT, flags: DRAW_TEXT_FORMAT) -> Option<Vec<u8>> {
    unsafe {
        let mem = CreateCompatibleDC(None);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(mem), &bitmap_info(w, h), DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));
        let old_font = SelectObject(mem, HGDIOBJ(font.0));
        SetBkMode(mem, TRANSPARENT);
        SetTextColor(mem, COLORREF(0x00FF_FFFF));
        let mut buf = wide(text);
        let len = buf.len() - 1;
        let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
        DrawTextW(mem, &mut buf[..len], &mut r, flags | DT_NOPREFIX);
        let _ = GdiFlush();
        let px = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
        let mask = px.as_chunks::<4>().0.iter().map(|p| p[0].max(p[1]).max(p[2])).collect();
        SelectObject(mem, old_font);
        SelectObject(mem, old_bmp);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        Some(mask)
    }
}

/// Width and height `text` needs in `font` on one line.
pub fn measure(text: &str, font: HFONT) -> (i32, i32) {
    unsafe {
        let mem = CreateCompatibleDC(None);
        let old = SelectObject(mem, HGDIOBJ(font.0));
        let mut buf = wide(text);
        let len = buf.len() - 1;
        let mut r = RECT::default();
        DrawTextW(mem, &mut buf[..len], &mut r, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(mem, old);
        let _ = DeleteDC(mem);
        (r.right - r.left, r.bottom - r.top)
    }
}

/// A grayscale-antialiased UI font (ClearType's coloured fringes can't be
/// turned into alpha), `px` pixels tall.
pub fn font(px: i32, semibold: bool) -> HFONT {
    let face = wide("Segoe UI");
    unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            if semibold { FW_SEMIBOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
            0,
            0,
            0,
            FONT_CHARSET(1), // DEFAULT_CHARSET
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(ANTIALIASED_QUALITY.0),
            0,
            PCWSTR(face.as_ptr()),
        )
    }
}

/// Loads an icon as a premultiplied RGBA pixmap.
pub fn icon_pixmap(source: &IconSource, size: i32) -> Option<Pixmap> {
    let bmp = icons::load(source, size)?;
    let pix = pixmap_from_bitmap(bmp, size);
    icons::free(bmp);
    pix
}

fn pixmap_from_bitmap(bmp: HBITMAP, size: i32) -> Option<Pixmap> {
    let mut info = bitmap_info(size, size);
    let mut buf = vec![0u8; (size * size * 4) as usize];
    unsafe {
        let dc: HDC = GetDC(None);
        let lines = GetDIBits(dc, bmp, 0, size as u32, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        if lines == 0 {
            return None;
        }
    }
    // Old 24-bit icons come back with no alpha at all: treat them as opaque.
    let has_alpha = buf.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    let mut pix = Pixmap::new(size as u32, size as u32)?;
    for (d, s) in pix.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(buf.as_chunks::<4>().0) {
        // BGRA → RGBA; keep premultiplied, and never let colour exceed alpha.
        let a = if has_alpha { s[3] } else { 255 };
        d[0] = s[2].min(a);
        d[1] = s[1].min(a);
        d[2] = s[0].min(a);
        d[3] = a;
    }
    Some(pix)
}
