//! A full screen look at the pictures on the line.
//!
//! The line itself is too small to read anything from, so a click on a card
//! opens this: the desktop dimmed, the picture as large as the screen allows,
//! its name and pixel size underneath. Left and right walk the line while it is
//! open, Escape or a click puts it away.
//!
//! Only one picture's pixels are held here at a time. Stepping asks the line
//! for the next one, because copying twelve 4K pictures into this window would
//! cost several hundred megabytes to show one of them.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::canvas::Canvas;
use crate::clipboard::Shot;
use crate::gdiplus::Painter;

/// Posted to the line's window when a preview closes, so the line can put
/// itself back on top of everything.
pub const CLOSED: u32 = WM_APP + 2;
/// Posted to the line when the arrow keys ask for another picture.
pub const STEP: u32 = WM_APP + 4;

/// The preview on screen, if any. Replacing one must not look like closing it,
/// so the window that goes away checks that it is still the current one.
static CURRENT: AtomicIsize = AtomicIsize::new(0);

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

struct State {
    canvas: Canvas,
    shot: Shot,
    /// Where this picture sits in the line, and how long the line is.
    index: usize,
    count: usize,
    scale: f32,
    owner: HWND,
    origin: (i32, i32),
    /// Whoever had the keyboard before the preview took it.
    previous: HWND,
}

/// Shows `shot` over the whole of `monitor`. Returns once it is up; the window
/// closes itself when it is clicked or Escape is pressed.
pub fn open(owner: HWND, shot: &Shot, index: usize, count: usize, monitor: RECT) -> bool {
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
            shot: copy(shot),
            index,
            count,
            scale,
            owner,
            origin: (monitor.left, monitor.top),
            previous: GetForegroundWindow(),
        }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        CURRENT.store(hwnd.0 as isize, Ordering::SeqCst);
        paint(&*state);

        // Activated rather than shown quietly: the arrow keys have to reach
        // this window, and a window that never takes focus never gets them.
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        true
    }
}

/// Replaces what the open preview is showing. Called when the line answers a
/// `STEP` request.
pub fn show(shot: &Shot, index: usize, count: usize) {
    let pointer = CURRENT.load(Ordering::SeqCst);
    if pointer == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(pointer as *mut std::ffi::c_void);
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if state.is_null() {
            return;
        }
        let state = &mut *state;
        state.shot = copy(shot);
        state.index = index;
        state.count = count;
        paint(state);
    }
}

/// A shot of our own: the preview outlives the call that opened it.
fn copy(shot: &Shot) -> Shot {
    Shot {
        width: shot.width,
        height: shot.height,
        pixels: shot.pixels.clone(),
        hash: shot.hash,
        path: shot.path.clone(),
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

    // "3 / 7 · 截图.png · 2560 × 1440" — where you are matters more than the
    // file name once there are several.
    let caption = if state.count > 1 {
        format!("{} / {} · {}", state.index + 1, state.count, state.shot.caption())
    } else {
        state.shot.caption()
    };
    painter.text_centered(
        &caption,
        image_x,
        image_y + image_h + pad * 2.0,
        image_w,
        caption_h,
        12.5 * scale,
        argb(235, 255, 255, 255),
    );
    let hint = if state.count > 1 {
        "← → 翻看　·　点任意位置或 Esc 收起"
    } else {
        "点任意位置或 Esc 收起"
    };
    painter.text_centered(
        hint,
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
        WM_KEYDOWN => {
            let key = wparam.0 as u16;
            match key {
                0x1B | 0x20 => {
                    // Escape, and Space as Quick Look on the Mac does it.
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
                _ if !pointer.is_null() => {
                    let state = &*pointer;
                    let wanted = match key {
                        0x25 => state.index.checked_sub(1),          // left
                        0x27 => Some(state.index + 1),                // right
                        0x24 => Some(0),                              // home
                        0x23 => Some(state.count.saturating_sub(1)),   // end
                        _ => None,
                    };
                    if let Some(next) = wanted {
                        if next < state.count && next != state.index {
                            // The pixels live in the line; ask it for them.
                            let _ = PostMessageW(
                                state.owner,
                                STEP,
                                WPARAM(next),
                                LPARAM(0),
                            );
                        }
                    }
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        WM_DESTROY => {
            if !pointer.is_null() {
                let state = Box::from_raw(pointer);
                let owner = state.owner;
                let previous = state.previous;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(state);
                // Give the keyboard back to whoever had it.
                if !previous.is_invalid() {
                    let _ = SetForegroundWindow(previous);
                }
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
