//! A window painted through a 32-bit DIB section and `UpdateLayeredWindow`,
//! which is what gives per-pixel transparency and soft edges on Windows.

use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    AC_SRC_ALPHA, AC_SRC_OVER,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

pub struct Canvas {
    hwnd: HWND,
    screen: HDC,
    mem: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    pub width: i32,
    pub height: i32,
}

impl Canvas {
    pub fn new(hwnd: HWND, width: i32, height: i32) -> Option<Canvas> {
        if width <= 0 || height <= 0 {
            return None;
        }
        unsafe {
            let screen = GetDC(None);
            if screen.0.is_null() {
                return None;
            }
            let mem = CreateCompatibleDC(screen);
            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width;
            // Negative height: rows are stored top down, matching the drawing
            // coordinates the rest of the code uses.
            info.bmiHeader.biHeight = -height;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB.0;

            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(mem, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) => bitmap,
                Err(_) => {
                    let _ = DeleteDC(mem);
                    ReleaseDC(None, screen);
                    return None;
                }
            };
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(mem);
                ReleaseDC(None, screen);
                return None;
            }
            let old = SelectObject(mem, HGDIOBJ(bitmap.0));
            Some(Canvas {
                hwnd,
                screen,
                mem,
                bitmap,
                old,
                bits: bits as *mut u32,
                width,
                height,
            })
        }
    }

    pub fn hdc(&self) -> HDC {
        self.mem
    }

    /// Every frame starts from nothing: the strip is transparent wherever
    /// nothing is drawn.
    pub fn clear(&self) {
        unsafe {
            std::ptr::write_bytes(self.bits, 0, (self.width * self.height) as usize);
        }
    }

    /// Puts the strip on screen with its top left corner at `x, y`.
    pub fn present(&self, x: i32, y: i32) {
        unsafe {
            let dst = POINT { x, y };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                self.screen,
                Some(&dst),
                Some(&size),
                self.mem,
                Some(&src),
                windows::Win32::Foundation::COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.mem);
            ReleaseDC(None, self.screen);
        }
    }
}
