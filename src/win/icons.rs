//! Icon loading. Every icon ends up as a 32-bit premultiplied-alpha DIB
//! (`HBITMAP`), which popup menus and image lists both draw with transparency.
//!
//! - Apps: `IShellItemImageFactory`, i.e. the same icon Start shows, served from
//!   Windows' own icon cache.
//! - Custom icons: PNG/ICO/JPG/BMP/GIF through WIC, SVG through resvg. Picked
//!   files are copied into the data folder's `icons\` directory so the
//!   configuration stays portable.
//!
//! App icons are loaded on a background thread and handed to the UI in batches.

use super::ui::wide;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{GENERIC_READ, HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP, HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapFrameDecode, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::PCWSTR;

pub const ICON_EXTENSIONS: &[&str] = &["png", "ico", "svg", "jpg", "jpeg", "bmp", "gif"];

#[derive(Clone, Debug)]
pub enum Source {
    /// Parsing name inside shell:AppsFolder.
    Shell(String),
    /// Any file or folder path; its shell icon.
    Path(String),
    /// An image file (custom icon).
    Image(PathBuf),
}

/// Loads one icon. Needs COM initialized on the calling thread.
pub fn load(source: &Source, size: i32) -> Option<HBITMAP> {
    match source {
        Source::Shell(pn) => shell_icon(&format!("shell:AppsFolder\\{pn}"), size),
        Source::Path(p) => shell_icon(p, size),
        Source::Image(p) => image_icon(p, size),
    }
}

fn shell_icon(path: &str, size: i32) -> Option<HBITMAP> {
    let w = wide(path);
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY).ok()
    }
}

fn image_icon(path: &Path, size: i32) -> Option<HBITMAP> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let pixels = if ext == "svg" { svg_pixels(path, size)? } else { wic_pixels(path, size)? };
    dib_from_bgra(&pixels, size)
}

/// Decodes with WIC into premultiplied BGRA, scaled to fit a `size`×`size` square.
fn wic_pixels(path: &Path, size: i32) -> Option<Vec<u8>> {
    let w = wide(&path.display().to_string());
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory
            .CreateDecoderFromFilename(PCWSTR(w.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .ok()?;
        // .ico files hold several sizes: take the largest frame and scale down.
        let count = decoder.GetFrameCount().ok()?;
        let mut best: Option<(IWICBitmapFrameDecode, u32)> = None;
        for i in 0..count {
            let Ok(frame) = decoder.GetFrame(i) else { continue };
            let (mut fw, mut fh) = (0u32, 0u32);
            if frame.GetSize(&mut fw, &mut fh).is_err() {
                continue;
            }
            if best.as_ref().is_none_or(|(_, area)| fw * fh > *area) {
                best = Some((frame, fw * fh));
            }
        }
        let (frame, _) = best?;
        let (mut fw, mut fh) = (0u32, 0u32);
        frame.GetSize(&mut fw, &mut fh).ok()?;
        if fw == 0 || fh == 0 {
            return None;
        }
        let (sw, sh) = fit(fw, fh, size as u32);

        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, sw, sh, WICBitmapInterpolationModeHighQualityCubic).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let stride = sw * 4;
        let mut scaled = vec![0u8; (stride * sh) as usize];
        converter.CopyPixels(std::ptr::null(), stride, &mut scaled).ok()?;
        Some(center(&scaled, sw, sh, size as u32))
    }
}

fn svg_pixels(path: &Path, size: i32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size as u32, size as u32)?;
    let svg = tree.size();
    let s = (size as f32 / svg.width()).min(size as f32 / svg.height());
    let dx = (size as f32 - svg.width() * s) / 2.0;
    let dy = (size as f32 - svg.height() * s) / 2.0;
    resvg::render(&tree, tiny_skia::Transform::from_row(s, 0.0, 0.0, s, dx, dy), &mut pixmap.as_mut());
    // tiny-skia is premultiplied RGBA; DIBs want BGRA.
    let mut px = pixmap.take();
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    Some(px)
}

fn fit(w: u32, h: u32, size: u32) -> (u32, u32) {
    if w >= h { (size, (h * size / w).max(1)) } else { ((w * size / h).max(1), size) }
}

fn center(src: &[u8], w: u32, h: u32, size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    let (ox, oy) = ((size - w) / 2, (size - h) / 2);
    for y in 0..h {
        let s = (y * w * 4) as usize;
        let d = (((y + oy) * size + ox) * 4) as usize;
        out[d..d + (w * 4) as usize].copy_from_slice(&src[s..s + (w * 4) as usize]);
    }
    out
}

fn dib_from_bgra(pixels: &[u8], size: i32) -> Option<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            return None;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len().min((size * size * 4) as usize));
        Some(bmp)
    }
}

/// An invisible icon-sized bitmap. Alpha is 1 rather than 0: image lists treat
/// an all-zero alpha channel as "no alpha" and would draw it as solid black.
pub fn blank(size: i32) -> Option<HBITMAP> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    for p in px.chunks_exact_mut(4) {
        p[3] = 1;
    }
    dib_from_bgra(&px, size)
}

pub fn free(bmp: HBITMAP) {
    if !bmp.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
        }
    }
}

/// Copies a picked image into `icons\` under a unique name and returns that
/// name. The original file can then be moved or deleted without breaking the icon.
pub fn import_file(src: &Path, icons_dir: &Path) -> Result<String, String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !ICON_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("Unsupported icon type: .{ext}"));
    }
    let len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > 8 * 1024 * 1024 {
        return Err("That image is larger than 8 MB; please pick a smaller icon.".into());
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let name = format!("{stamp:x}.{ext}");
    std::fs::copy(src, icons_dir.join(&name)).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Loads `requests` on a background thread and posts results to `hwnd` as
/// `msg` with a `Box<Vec<(String, HBITMAP)>>` in LPARAM, in batches.
pub fn load_async(requests: Vec<(String, Source)>, size: i32, hwnd: HWND, msg: u32) {
    let hwnd_raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let hwnd = HWND(hwnd_raw as *mut _);
        let mut batch: Vec<(String, isize)> = Vec::new();
        let flush = |batch: &mut Vec<(String, isize)>| {
            if batch.is_empty() {
                return;
            }
            let boxed = Box::new(std::mem::take(batch));
            let ptr = Box::into_raw(boxed);
            if unsafe { PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(ptr as isize)) }.is_err() {
                // Window gone: reclaim and free.
                let items = unsafe { Box::from_raw(ptr) };
                for (_, b) in items.iter() {
                    free(HBITMAP(*b as *mut _));
                }
            }
        };
        for (key, source) in requests {
            if let Some(bmp) = load(&source, size) {
                batch.push((key, bmp.0 as isize));
            }
            if batch.len() >= 48 {
                flush(&mut batch);
            }
        }
        flush(&mut batch);
    });
}
