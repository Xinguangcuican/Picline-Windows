//! The slice of GDI+ this app needs: anti-aliased curves and rounded boxes,
//! a line of text, and enough image handling to turn a clipboard blob into
//! pixels and back again. Declared by hand so the build does not depend on
//! the crate's GdiPlus bindings, which come and go between releases.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Graphics::Gdi::HDC;

#[repr(C)]
struct StartupInput {
    version: u32,
    callback: *mut c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[repr(C)]
struct RectF {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[repr(C)]
#[derive(Default)]
struct RectI {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

#[repr(C)]
#[derive(Default)]
struct BitmapData {
    width: u32,
    height: u32,
    stride: i32,
    format: i32,
    scan0: *mut c_void,
    reserved: *mut c_void,
}

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> i32;
    fn GdipCreateFromHDC(hdc: HDC, graphics: *mut *mut c_void) -> i32;
    fn GdipDeleteGraphics(graphics: *mut c_void) -> i32;
    fn GdipSetSmoothingMode(graphics: *mut c_void, mode: i32) -> i32;
    fn GdipSetTextRenderingHint(graphics: *mut c_void, hint: i32) -> i32;
    fn GdipSetInterpolationMode(graphics: *mut c_void, mode: i32) -> i32;
    fn GdipCreatePath(mode: i32, path: *mut *mut c_void) -> i32;
    fn GdipAddPathBezier(
        path: *mut c_void,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        x4: f32,
        y4: f32,
    ) -> i32;
    fn GdipAddPathLine(path: *mut c_void, x1: f32, y1: f32, x2: f32, y2: f32) -> i32;
    fn GdipAddPathRectangle(path: *mut c_void, x: f32, y: f32, width: f32, height: f32) -> i32;
    fn GdipAddPathArc(
        path: *mut c_void,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        start: f32,
        sweep: f32,
    ) -> i32;
    fn GdipClosePathFigure(path: *mut c_void) -> i32;
    fn GdipDeletePath(path: *mut c_void) -> i32;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut *mut c_void) -> i32;
    fn GdipDeletePen(pen: *mut c_void) -> i32;
    fn GdipSetPenStartCap(pen: *mut c_void, cap: i32) -> i32;
    fn GdipSetPenEndCap(pen: *mut c_void, cap: i32) -> i32;
    fn GdipDrawPath(graphics: *mut c_void, pen: *mut c_void, path: *mut c_void) -> i32;
    fn GdipCreateSolidFill(color: u32, brush: *mut *mut c_void) -> i32;
    fn GdipDeleteBrush(brush: *mut c_void) -> i32;
    fn GdipFillPath(graphics: *mut c_void, brush: *mut c_void, path: *mut c_void) -> i32;
    fn GdipCreateFontFamilyFromName(
        name: *const u16,
        collection: *mut c_void,
        family: *mut *mut c_void,
    ) -> i32;
    fn GdipDeleteFontFamily(family: *mut c_void) -> i32;
    fn GdipCreateFont(
        family: *mut c_void,
        size: f32,
        style: i32,
        unit: i32,
        font: *mut *mut c_void,
    ) -> i32;
    fn GdipDeleteFont(font: *mut c_void) -> i32;
    fn GdipCreateStringFormat(flags: i32, lang: u16, format: *mut *mut c_void) -> i32;
    fn GdipDeleteStringFormat(format: *mut c_void) -> i32;
    fn GdipSetStringFormatAlign(format: *mut c_void, align: i32) -> i32;
    fn GdipSetStringFormatLineAlign(format: *mut c_void, align: i32) -> i32;
    fn GdipDrawString(
        graphics: *mut c_void,
        string: *const u16,
        length: i32,
        font: *mut c_void,
        layout: *const RectF,
        format: *mut c_void,
        brush: *mut c_void,
    ) -> i32;

    fn GdipCreateBitmapFromScan0(
        width: i32,
        height: i32,
        stride: i32,
        format: i32,
        scan0: *mut u8,
        bitmap: *mut *mut c_void,
    ) -> i32;
    fn GdipCreateBitmapFromStream(stream: *mut c_void, bitmap: *mut *mut c_void) -> i32;
    fn GdipCreateBitmapFromFile(filename: *const u16, bitmap: *mut *mut c_void) -> i32;
    fn GdipDisposeImage(image: *mut c_void) -> i32;
    fn GdipGetImageWidth(image: *mut c_void, width: *mut u32) -> i32;
    fn GdipGetImageHeight(image: *mut c_void, height: *mut u32) -> i32;
    fn GdipGetImageGraphicsContext(image: *mut c_void, graphics: *mut *mut c_void) -> i32;
    fn GdipDrawImageRect(
        graphics: *mut c_void,
        image: *mut c_void,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> i32;
    fn GdipBitmapLockBits(
        image: *mut c_void,
        rect: *const RectI,
        flags: u32,
        format: i32,
        data: *mut BitmapData,
    ) -> i32;
    fn GdipBitmapUnlockBits(image: *mut c_void, data: *mut BitmapData) -> i32;
    fn GdipSaveImageToFile(
        image: *mut c_void,
        filename: *const u16,
        clsid: *const windows::core::GUID,
        params: *const c_void,
    ) -> i32;
}

#[link(name = "shlwapi")]
extern "system" {
    fn SHCreateMemStream(init: *const u8, size: u32) -> *mut c_void;
}

const UNIT_PIXEL: i32 = 2;
const SMOOTHING_ANTIALIAS: i32 = 4;
const CAP_ROUND: i32 = 2;
const ALIGN_CENTER: i32 = 1;
const INTERPOLATION_BICUBIC: i32 = 7;
/// 32 bits per pixel, alpha, red, green, blue — the same byte order as the
/// DIB sections the canvas paints into, so buffers move between them as is.
const FORMAT_ARGB32: i32 = 0x0026_200A;
const LOCK_READ: u32 = 0x1;

/// Decoded pixels, top down, four bytes per pixel, straight from a file or a
/// stream. Everything the app keeps in memory ends up in this shape.
pub struct Raw {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// One drawing pass over a device context. Made and dropped per frame; the
/// objects inside live only as long as the pass.
pub struct Painter {
    graphics: *mut c_void,
}

impl Painter {
    pub fn new(hdc: HDC) -> Option<Painter> {
        unsafe {
            let mut graphics: *mut c_void = std::ptr::null_mut();
            if GdipCreateFromHDC(hdc, &mut graphics) != 0 || graphics.is_null() {
                return None;
            }
            let _ = GdipSetSmoothingMode(graphics, SMOOTHING_ANTIALIAS);
            let _ = GdipSetTextRenderingHint(graphics, 4);
            let _ = GdipSetInterpolationMode(graphics, INTERPOLATION_BICUBIC);
            Some(Painter { graphics })
        }
    }

    /// A cubic curve, drawn with a round cap so the ends never look cut off.
    #[allow(clippy::too_many_arguments)]
    pub fn curve(&self, from: (f32, f32), to: (f32, f32), control: (f32, f32), width: f32, argb: u32) {
        unsafe {
            let mut path: *mut c_void = std::ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            // Control points that make the cubic trace the same parabola as a
            // quadratic with `control`: 2/3 of the way from each end.
            let c1 = (from.0 + 2.0 / 3.0 * (control.0 - from.0), from.1 + 2.0 / 3.0 * (control.1 - from.1));
            let c2 = (to.0 + 2.0 / 3.0 * (control.0 - to.0), to.1 + 2.0 / 3.0 * (control.1 - to.1));
            let _ = GdipAddPathBezier(path, from.0, from.1, c1.0, c1.1, c2.0, c2.1, to.0, to.1);

            let mut pen: *mut c_void = std::ptr::null_mut();
            if GdipCreatePen1(argb, width, UNIT_PIXEL, &mut pen) == 0 {
                let _ = GdipSetPenStartCap(pen, CAP_ROUND);
                let _ = GdipSetPenEndCap(pen, CAP_ROUND);
                let _ = GdipDrawPath(self.graphics, pen, path);
                let _ = GdipDeletePen(pen);
            }
            let _ = GdipDeletePath(path);
        }
    }

    pub fn line(&self, from: (f32, f32), to: (f32, f32), width: f32, argb: u32) {
        unsafe {
            let mut path: *mut c_void = std::ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            let _ = GdipAddPathLine(path, from.0, from.1, to.0, to.1);
            let mut pen: *mut c_void = std::ptr::null_mut();
            if GdipCreatePen1(argb, width, UNIT_PIXEL, &mut pen) == 0 {
                let _ = GdipSetPenStartCap(pen, CAP_ROUND);
                let _ = GdipSetPenEndCap(pen, CAP_ROUND);
                let _ = GdipDrawPath(self.graphics, pen, path);
                let _ = GdipDeletePen(pen);
            }
            let _ = GdipDeletePath(path);
        }
    }

    pub fn capsule(&self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        self.rounded(x, y, w, h, h / 2.0, argb);
    }

    /// Filled rounded rectangle. `radius` is clamped to half the short side.
    pub fn rounded(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, argb: u32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = radius.max(0.0).min(w / 2.0).min(h / 2.0);
        unsafe {
            let mut path: *mut c_void = std::ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            if r <= 0.5 {
                let _ = GdipAddPathRectangle(path, x, y, w, h);
            } else {
                let d = r * 2.0;
                let _ = GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
                let _ = GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
                let _ = GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
                let _ = GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            }
            let _ = GdipClosePathFigure(path);

            let mut brush: *mut c_void = std::ptr::null_mut();
            if GdipCreateSolidFill(argb, &mut brush) == 0 {
                let _ = GdipFillPath(self.graphics, brush, path);
                let _ = GdipDeleteBrush(brush);
            }
            let _ = GdipDeletePath(path);
        }
    }

    /// Just the edge of that rectangle.
    pub fn rounded_outline(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, width: f32, argb: u32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let r = radius.max(0.0).min(w / 2.0).min(h / 2.0);
        unsafe {
            let mut path: *mut c_void = std::ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            if r <= 0.5 {
                let _ = GdipAddPathRectangle(path, x, y, w, h);
            } else {
                let d = r * 2.0;
                let _ = GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
                let _ = GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
                let _ = GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
                let _ = GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            }
            let _ = GdipClosePathFigure(path);

            let mut pen: *mut c_void = std::ptr::null_mut();
            if GdipCreatePen1(argb, width, UNIT_PIXEL, &mut pen) == 0 {
                let _ = GdipDrawPath(self.graphics, pen, path);
                let _ = GdipDeletePen(pen);
            }
            let _ = GdipDeletePath(path);
        }
    }

    /// Draws a decoded image, scaled into the rectangle. The bitmap borrows
    /// the buffer for the length of the call and is thrown away after.
    pub fn image(&self, pixels: &[u8], width: u32, height: u32, x: f32, y: f32, w: f32, h: f32) {
        if width == 0 || height == 0 || pixels.len() < (width * height * 4) as usize {
            return;
        }
        unsafe {
            let mut bitmap: *mut c_void = std::ptr::null_mut();
            if GdipCreateBitmapFromScan0(
                width as i32,
                height as i32,
                (width * 4) as i32,
                FORMAT_ARGB32,
                pixels.as_ptr() as *mut u8,
                &mut bitmap,
            ) != 0
            {
                return;
            }
            let _ = GdipDrawImageRect(self.graphics, bitmap, x, y, w, h);
            let _ = GdipDisposeImage(bitmap);
        }
    }

    pub fn text_centered(&self, text: &str, x: f32, y: f32, w: f32, h: f32, size: f32, argb: u32) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let mut family: *mut c_void = std::ptr::null_mut();
            // YaHei carries Chinese; Segoe UI is the fallback for a system
            // without it (Windows 10 and 11 both ship YaHei).
            for name in ["Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI"] {
                let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                if GdipCreateFontFamilyFromName(name_wide.as_ptr(), std::ptr::null_mut(), &mut family) == 0
                    && !family.is_null()
                {
                    break;
                }
                family = std::ptr::null_mut();
            }
            if family.is_null() {
                return;
            }
            let mut font: *mut c_void = std::ptr::null_mut();
            if GdipCreateFont(family, size, 0, UNIT_PIXEL, &mut font) != 0 {
                let _ = GdipDeleteFontFamily(family);
                return;
            }
            let mut format: *mut c_void = std::ptr::null_mut();
            let _ = GdipCreateStringFormat(0, 0, &mut format);
            let _ = GdipSetStringFormatAlign(format, ALIGN_CENTER);
            let _ = GdipSetStringFormatLineAlign(format, ALIGN_CENTER);

            let mut brush: *mut c_void = std::ptr::null_mut();
            if GdipCreateSolidFill(argb, &mut brush) == 0 {
                let layout = RectF { x, y, w, h };
                let _ = GdipDrawString(
                    self.graphics,
                    wide.as_ptr(),
                    wide.len() as i32,
                    font,
                    &layout,
                    format,
                    brush,
                );
                let _ = GdipDeleteBrush(brush);
            }
            let _ = GdipDeleteStringFormat(format);
            let _ = GdipDeleteFont(font);
            let _ = GdipDeleteFontFamily(family);
        }
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        unsafe {
            let _ = GdipDeleteGraphics(self.graphics);
        }
    }
}

/// Started once, before the first window is drawn.
pub struct GdiPlus {
    token: usize,
}

impl GdiPlus {
    pub fn start() -> Option<GdiPlus> {
        unsafe {
            let input = StartupInput {
                version: 1,
                callback: std::ptr::null_mut(),
                suppress_background_thread: 0,
                suppress_external_codecs: 0,
            };
            let mut token = 0usize;
            if GdiplusStartup(&mut token, &input, std::ptr::null_mut()) != 0 {
                return None;
            }
            Some(GdiPlus { token })
        }
    }
}

impl Drop for GdiPlus {
    fn drop(&mut self) {
        // GDI+ runs for the life of the process; shutting it down while other
        // threads may still draw is not worth the risk.
        let _ = self.token;
    }
}

// --- decoding -------------------------------------------------------------

/// PNG bytes from the clipboard. Some apps offer nothing else.
pub fn image_from_stream(bytes: &[u8]) -> Option<Raw> {
    if bytes.is_empty() {
        return None;
    }
    unsafe {
        let stream = SHCreateMemStream(bytes.as_ptr(), bytes.len() as u32);
        if stream.is_null() {
            return None;
        }
        let mut image: *mut c_void = std::ptr::null_mut();
        let status = GdipCreateBitmapFromStream(stream, &mut image);
        release_com(stream);
        if status != 0 || image.is_null() {
            return None;
        }
        let raw = copy_out(image, MAX_SIDE);
        let _ = GdipDisposeImage(image);
        raw
    }
}

/// A picture sitting on disk, which is what copying a file in Explorer gives.
pub fn image_from_file(path: &[u16]) -> Option<Raw> {
    unsafe {
        let mut image: *mut c_void = std::ptr::null_mut();
        if GdipCreateBitmapFromFile(path.as_ptr(), &mut image) != 0 || image.is_null() {
            return None;
        }
        let raw = copy_out(image, MAX_SIDE);
        let _ = GdipDisposeImage(image);
        raw
    }
}

/// Writes the buffer out as a PNG. Used for saving a copy and for the
/// temporary file that a plain screenshot needs before it can be pasted into a
/// chat as a file.
pub fn save_png(pixels: &[u8], width: u32, height: u32, path: &std::path::Path) -> bool {
    if width == 0 || height == 0 || pixels.len() < (width * height * 4) as usize {
        return false;
    }
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut bitmap: *mut c_void = std::ptr::null_mut();
        if GdipCreateBitmapFromScan0(
            width as i32,
            height as i32,
            (width * 4) as i32,
            FORMAT_ARGB32,
            pixels.as_ptr() as *mut u8,
            &mut bitmap,
        ) != 0
        {
            return false;
        }
        let status = GdipSaveImageToFile(bitmap, wide.as_ptr(), &PNG_ENCODER, std::ptr::null());
        let _ = GdipDisposeImage(bitmap);
        status == 0
    }
}

/// `image/png`, the encoder GDI+ carries on every Windows since XP.
const PNG_ENCODER: windows::core::GUID = windows::core::GUID::from_u128(0x557C_F406_1A04_11D3_9A73_0000F81E_F32E);

/// Brings a picture down to a size worth keeping in memory.
pub fn shrink(width: u32, height: u32, pixels: &[u8]) -> Option<Raw> {
    if width == 0 || height == 0 {
        return None;
    }
    unsafe {
        let mut source: *mut c_void = std::ptr::null_mut();
        if GdipCreateBitmapFromScan0(
            width as i32,
            height as i32,
            (width * 4) as i32,
            FORMAT_ARGB32,
            pixels.as_ptr() as *mut u8,
            &mut source,
        ) != 0
        {
            return None;
        }
        let raw = copy_out(source, MAX_SIDE);
        let _ = GdipDisposeImage(source);
        raw
    }
}

/// Anything longer than this on its longer side is scaled down before it is
/// kept: twelve screen sized pictures would otherwise be hundreds of megabytes.
const MAX_SIDE: u32 = 1400;

unsafe fn copy_out(image: *mut c_void, max_side: u32) -> Option<Raw> {
    let mut width = 0u32;
    let mut height = 0u32;
    if GdipGetImageWidth(image, &mut width) != 0 || GdipGetImageHeight(image, &mut height) != 0 {
        return None;
    }
    if width == 0 || height == 0 {
        return None;
    }
    let factor = (max_side as f32 / width.max(height) as f32).min(1.0);
    let target_w = ((width as f32 * factor).round() as u32).max(1);
    let target_h = ((height as f32 * factor).round() as u32).max(1);

    let mut target: *mut c_void = std::ptr::null_mut();
    if GdipCreateBitmapFromScan0(
        target_w as i32,
        target_h as i32,
        0,
        FORMAT_ARGB32,
        std::ptr::null_mut(),
        &mut target,
    ) != 0
    {
        return None;
    }
    let mut graphics: *mut c_void = std::ptr::null_mut();
    if GdipGetImageGraphicsContext(target, &mut graphics) != 0 {
        let _ = GdipDisposeImage(target);
        return None;
    }
    let _ = GdipSetInterpolationMode(graphics, INTERPOLATION_BICUBIC);
    let _ = GdipDrawImageRect(graphics, image, 0.0, 0.0, target_w as f32, target_h as f32);
    let _ = GdipDeleteGraphics(graphics);

    let rect = RectI {
        x: 0,
        y: 0,
        w: target_w as i32,
        h: target_h as i32,
    };
    let mut data = BitmapData::default();
    let mut pixels = vec![0u8; (target_w * target_h * 4) as usize];
    if GdipBitmapLockBits(target, &rect, LOCK_READ, FORMAT_ARGB32, &mut data) != 0 {
        let _ = GdipDisposeImage(target);
        return None;
    }
    let row_bytes = (target_w * 4) as usize;
    let base = data.scan0 as *const u8;
    for y in 0..target_h as usize {
        let row = base.offset(y as isize * data.stride as isize);
        std::ptr::copy_nonoverlapping(row, pixels.as_mut_ptr().add(y * row_bytes), row_bytes);
    }
    let _ = GdipBitmapUnlockBits(target, &mut data);
    let _ = GdipDisposeImage(target);

    Some(Raw {
        width: target_w,
        height: target_h,
        pixels,
    })
}

/// Drops one reference on a COM object: `Release` is the third vtable slot.
unsafe fn release_com(object: *mut c_void) {
    if object.is_null() {
        return;
    }
    let vtable = *(object as *const *const *const c_void);
    if vtable.is_null() {
        return;
    }
    let release: extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtable.add(2));
    release(object);
}
