//! A full screen look at one picture.
//!
//! The line itself is too small to read anything from, so a click on a card
//! opens this: the desktop dimmed, the picture as large as the screen allows,
//! its name and pixel size underneath. Any click, or Escape, puts it away.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::canvas::Canvas;
use crate::clipboard::Shot;
use crate::gdiplus::Painter;

/// Posted to the line's window when a preview closes, so the line can put
/// itself back on top of everything.
pub const CLOSED: u32 = WM_APP + 2;

/// The preview on screen, if any. Replacing one must not look like closing it,
/// so the window that goes away checks that it is still the current one.
static CURRENT: AtomicIsize = AtomicIsize::new(0);

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

struct State {
    canvas: Canvas,
    shot: Shot,
    scale: f32,
    owner: HWND,
    origin: (i32, i32),
}

/// Shows `shot` over the whole of `monitor`. Returns once it is up; the window
/// closes itself when it is clicked.
pub fn open(owner: HWND, shot: &Shot, monitor: RECT) -> bool {
    unsafe {
        // One at a time: clicking a second card replaces the first preview.
        // Clearing the marker first means the old window does not report a
        // close on its way out.
        if let Ok(stale) = FindWindowW(w!("PiclinePreview"), PCWSTR::null()) {
            CURRENT.store(0, Ordering::SeqCst);
            let _ = DestroyWindow(stale);
        }

        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PiclinePreview");
        let definition = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(instance.0),
            hbrBackground: HBRUSH::default(),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassW(&definition);

        let width = monitor.right - monitor.left;
        let height = monitor.bottom - monitor.top;
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class,
            w!("Picline preview"),
            WS_POPUP,
            monitor.left,
            monitor.top,
            width,
            height,
            None,
            None,
            HINSTANCE(instance.0),
            None,
        ) else {
            return false;
        };

        let Some(canvas) = Canvas::new(hwnd, width, height) else {
            let _ = DestroyWindow(hwnd);
            return false;
        };
        let scale = (height as f32 / 900.0).clamp(0.85, 2.0);
        let state = Box::into_raw(Box::new(State {
            canvas,
            shot: Shot {
                width: shot.width,
                height: shot.height,
                pixels: shot.pixels.clone(),
                hash: shot.hash,
                path: shot.path.clone(),
            },
            scale,
            owner,
            origin: (monitor.left, monitor.top),
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        CURRENT.store(hwnd.0 as isize, Ordering::SeqCst);
        paint(&*state);

        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        true
    }
}

fn paint(state: &State) {
    let canvas = &state.canvas;
    let width = canvas.width as f32;
    let height = canvas.height as f32;
    let scale = state.scale;
    state.canvas.clear();
    let Some(painter) = Painter::new(canvas.hdc()) else {
        return;
    };

    // Dim the desktop rather than hide it: the picture floats on top of what
    // is already there, and the whole screen becomes one big close button.
    painter.rounded(0.0, 0.0, width, height, 0.0, argb(112, 0, 0, 0));

    let caption_h = 34.0 * scale;
    let margin = 0.10;
    let box_w = width * (1.0 - margin * 1.6);
    let box_h = height * (1.0 - margin) - caption_h;
    let aspect = state.shot.aspect();
    let mut image_w = box_w;
    let mut image_h = image_w / aspect;
    if image_h > box_h {
        image_h = box_h;
        image_w = image_h * aspect;
    }
    let image_x = (width - image_w) / 2.0;
    let image_y = (height - caption_h - image_h) / 2.0;

    let pad = 6.0 * scale;
    painter.rounded(
        image_x - pad,
        image_y - pad,
        image_w + pad * 2.0,
        image_h + pad * 2.0,
        10.0 * scale,
        argb(255, 26, 26, 28),
    );
    painter.image(
        &state.shot.pixels,
        state.shot.width,
        state.shot.height,
        image_x,
        image_y,
        image_w,
        image_h,
    );
    painter.rounded_outline(
        image_x - pad,
        image_y - pad,
        image_w + pad * 2.0,
        image_h + pad * 2.0,
        10.0 * scale,
        1.0 * scale,
        argb(90, 255, 255, 255),
    );

    let caption = state.shot.caption();
    painter.text_centered(
        &caption,
        image_x,
        image_y + image_h + pad * 2.0,
        image_w,
        caption_h,
        12.5 * scale,
        argb(235, 255, 255, 255),
    );
    painter.text_centered(
        "点任意位置收起 · 右键卡片可复制或另存",
        0.0,
        height - 34.0 * scale,
        width,
        26.0 * scale,
        11.0 * scale,
        argb(110, 255, 255, 255),
    );

    canvas.present(state.origin.0, state.origin.1);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    match msg {
        // Closing on the way up, not on the way down: the rest of the click
        // then belongs to nobody, instead of falling through to a window
        // underneath.
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_KEYDOWN if wparam.0 == 0x1B => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        WM_DESTROY => {
            if !pointer.is_null() {
                let state = Box::from_raw(pointer);
                let owner = state.owner;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(state);
                if CURRENT.swap(0, Ordering::SeqCst) == hwnd.0 as isize {
                    let _ = PostMessageW(owner, CLOSED, WPARAM(0), LPARAM(0));
                }
            }
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
