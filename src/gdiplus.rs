//! The thin slice of GDI+ we need: an anti-aliased curve, a rounded capsule
//! and a line of text. Declared by hand so the build does not depend on the
//! crate's GdiPlus bindings, which come and go between releases.

use std::ffi::c_void;
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

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const StartupInput, output: *mut c_void) -> i32;
    fn GdipCreateFromHDC(hdc: HDC, graphics: *mut *mut c_void) -> i32;
    fn GdipDeleteGraphics(graphics: *mut c_void) -> i32;
    fn GdipSetSmoothingMode(graphics: *mut c_void, mode: i32) -> i32;
    fn GdipSetTextRenderingHint(graphics: *mut c_void, hint: i32) -> i32;
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
}

const UNIT_PIXEL: i32 = 2;
const SMOOTHING_ANTIALIAS: i32 = 4;
const CAP_ROUND: i32 = 2;
const ALIGN_CENTER: i32 = 1;

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

    pub fn capsule(&self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        unsafe {
            let mut path: *mut c_void = std::ptr::null_mut();
            if GdipCreatePath(0, &mut path) != 0 {
                return;
            }
            // Left cap, top edge, right cap, bottom edge: a rounded rectangle
            // whose ends are half circles.
            let _ = GdipAddPathArc(path, x, y, h, h, 90.0, 180.0);
            let _ = GdipAddPathArc(path, x + w - h, y, h, h, 270.0, 180.0);
            let _ = GdipClosePathFigure(path);

            let mut brush: *mut c_void = std::ptr::null_mut();
            if GdipCreateSolidFill(argb, &mut brush) == 0 {
                let _ = GdipFillPath(self.graphics, brush, path);
                let _ = GdipDeleteBrush(brush);
            }
            let _ = GdipDeletePath(path);
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
