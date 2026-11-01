//! Picline for Windows — first slice.
//!
//! What this build does: watch the pointer, and when it rests against the top
//! edge of a screen for a quarter of a second, slide a bare rope down from
//! behind the edge of that screen; move away and it goes back up. It is the
//! experiment we said we would do first — nothing else — to find out whether
//! the top edge can be used without getting in the way of a maximised
//! window's buttons.
//!
//! Keys: Ctrl+Alt+T brings it down or puts it away, Ctrl+Alt+Shift+Q quits.
//! The tray icon does the same two things with the mouse.

#![windows_subsystem = "windows"]

mod canvas;
mod gdiplus;

use std::time::{Duration, Instant};

use canvas::Canvas;
use gdiplus::{GdiPlus, Painter};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, HBRUSH, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// How close to the top counts as touching it.
const BAND: i32 = 3;
/// Resting this long against the edge brings the rope down. A quick trip to a
/// maximised window's buttons must not.
const REVEAL_DELAY: Duration = Duration::from_millis(250);
/// Away this long and it goes back up.
const RETRACT_DELAY: Duration = Duration::from_millis(500);
/// One slide, either way, in seconds.
const SLIDE: f32 = 0.20;
/// How far below the strip the pointer still counts as "on the line".
const ZONE_SLACK: i32 = 24;

const PANEL_HEIGHT: f32 = 210.0;
const TIMER_TICK: usize = 1;
const HOTKEY_TOGGLE: i32 = 1;
const HOTKEY_QUIT: i32 = 2;
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const MENU_TOGGLE: usize = 1;
const MENU_QUIT: usize = 2;

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

struct App {
    hwnd: HWND,
    /// The screen the line belongs to right now.
    monitor: RECT,
    scale: f32,
    canvas: Option<Canvas>,
    painted: bool,
    /// 0 = tucked above the edge, 1 = fully down.
    anim: f32,
    target: f32,
    shown: bool,
    hot_since: Option<Instant>,
    away_since: Option<Instant>,
    /// Opened on purpose with the key or the tray, so it stays while empty.
    pinned: bool,
    last_tick: Instant,
}

impl App {
    fn new(hwnd: HWND) -> App {
        App {
            hwnd,
            monitor: RECT::default(),
            scale: 1.0,
            canvas: None,
            painted: false,
            anim: 0.0,
            target: 0.0,
            shown: false,
            hot_since: None,
            away_since: None,
            pinned: false,
            last_tick: Instant::now(),
        }
    }

    fn height(&self) -> i32 {
        self.canvas.as_ref().map(|c| c.height).unwrap_or(0)
    }

    /// Sizes the strip for that screen: full width, one panel tall, scaled by
    /// the screen's own DPI so it looks the same on a 150% display.
    fn prepare(&mut self, monitor: RECT) {
        self.monitor = monitor;
        let mut dx = 96u32;
        let mut dy = 96u32;
        let hmon = unsafe { MonitorFromRect(&monitor, MONITOR_DEFAULTTONEAREST) };
        if unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_err() {
            dx = 96;
        }
        self.scale = dx as f32 / 96.0;

        let width = monitor.right - monitor.left;
        let height = (PANEL_HEIGHT * self.scale).round() as i32;
        let stale = self
            .canvas
            .as_ref()
            .map(|c| c.width != width || c.height != height)
            .unwrap_or(true);
        if stale {
            self.canvas = Canvas::new(self.hwnd, width, height);
            self.painted = false;
        }
    }

    fn paint(&mut self) {
        let Some(canvas) = self.canvas.as_ref() else { return };
        canvas.clear();
        let width = canvas.width as f32;
        let scale = self.scale;
        let top = 10.0 * scale;
        let sag = (30.0 * scale).min(width * 0.018);
        // The lowest point of the rope, which is where things hang from.
        let middle = top + sag;

        if let Some(painter) = Painter::new(canvas.hdc()) {
            let left = (-20.0, top);
            let right = (width + 20.0, top);
            let control = (width / 2.0, top + 2.0 * sag);

            // Soft shadow, core, then a thin highlight: the same three passes
            // the Mac version strokes, so the rope reads on light and dark
            // desktops alike.
            painter.curve(
                (left.0, left.1 + 1.2),
                (right.0, right.1 + 1.2),
                (control.0, control.1 + 1.2),
                1.6 * scale,
                argb(60, 0, 0, 0),
            );
            painter.curve(left, right, control, 1.2 * scale, argb(255, 140, 140, 140));
            painter.curve(
                (left.0, left.1 - 0.35),
                (right.0, right.1 - 0.35),
                (control.0, control.1 - 0.35),
                0.5 * scale,
                argb(115, 255, 255, 255),
            );

            let hint = "截图后，图片会挂到这里";
            let font = 12.0 * scale;
            let capsule_w = hint.chars().count() as f32 * font + 26.0 * scale;
            let capsule_h = font + 14.0 * scale;
            let capsule_x = width / 2.0 - capsule_w / 2.0;
            let capsule_y = middle + 34.0 * scale - capsule_h / 2.0;
            painter.capsule(capsule_x, capsule_y, capsule_w, capsule_h, argb(150, 28, 28, 28));
            painter.text_centered(
                hint,
                capsule_x,
                capsule_y - 1.0,
                capsule_w,
                capsule_h + 2.0,
                font,
                argb(235, 255, 255, 255),
            );
        }
        self.painted = true;
    }

    /// Puts the strip where the animation says it should be. Only the window
    /// position changes per frame; the picture is painted once.
    fn slide(&mut self) {
        if !self.painted {
            self.paint();
        }
        let Some(canvas) = self.canvas.as_ref() else { return };
        let eased = self.anim * self.anim * (3.0 - 2.0 * self.anim);
        let height = canvas.height as f32;
        let y = self.monitor.top - (height * (1.0 - eased)).round() as i32;

        if !self.shown {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    self.monitor.left,
                    y,
                    canvas.width,
                    canvas.height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
            self.shown = true;
        }
        canvas.present(self.monitor.left, y);
    }

    fn retract_to_zero(&mut self) {
        self.target = 0.0;
        self.pinned = false;
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().clamp(0.0, 0.1);
        self.last_tick = now;

        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let screen = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = GetMonitorInfoW(screen, &mut info);
        }
        let rc = info.rcMonitor;

        let against_edge = cursor.x >= rc.left
            && cursor.x < rc.right
            && cursor.y >= rc.top
            && cursor.y < rc.top + BAND;
        let on_the_line = cursor.x >= self.monitor.left
            && cursor.x < self.monitor.right
            && cursor.y >= self.monitor.top
            && cursor.y < self.monitor.top + self.height() + ZONE_SLACK;

        // Coming down: rest against the top edge long enough, and not while a
        // full screen app owns that screen.
        if self.target < 1.0 {
            if against_edge && !fullscreen_busy() {
                let since = *self.hot_since.get_or_insert(now);
                if now.duration_since(since) >= REVEAL_DELAY {
                    self.hot_since = None;
                    self.prepare(rc);
                    self.target = 1.0;
                    self.away_since = None;
                    self.pinned = false;
                }
            } else {
                self.hot_since = None;
            }
        }

        // Going back up.
        if self.target > 0.0 {
            if on_the_line {
                self.away_since = None;
                // Once you have come to the line, it answers to the pointer
                // again, so a pinned line still tucks itself away afterwards.
                self.pinned = false;
            } else {
                let since = *self.away_since.get_or_insert(now);
                if !self.pinned && now.duration_since(since) >= RETRACT_DELAY {
                    self.away_since = None;
                    self.retract_to_zero();
                }
            }
        }

        let step = dt / SLIDE;
        if (self.anim - self.target).abs() > 0.0001 {
            self.anim = if self.anim < self.target {
                (self.anim + step).min(self.target)
            } else {
                (self.anim - step).max(self.target)
            };
            self.slide();
        }

        if self.anim <= 0.0 && self.shown {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
            // The window is out of sight; let the canvas go so nothing is held
            // for a strip nobody is looking at.
            self.canvas = None;
            self.painted = false;
        }
    }

    fn toggle(&mut self) {
        if self.target > 0.0 {
            self.retract_to_zero();
            return;
        }
        let mut cursor = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut cursor);
        }
        let screen = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = GetMonitorInfoW(screen, &mut info);
        }
        self.prepare(info.rcMonitor);
        self.target = 1.0;
        self.pinned = true;
        self.away_since = None;
        self.slide();
    }

    unsafe fn add_tray(&self) {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            ..Default::default()
        };
        let tip: Vec<u16> = "Picline".encode_utf16().collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
    }

    unsafe fn remove_tray(&self) {
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }

    unsafe fn tray_menu(&self) {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_TOGGLE, w!("放下来 / 收起来"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("退出"));
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(self.hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            0,
            self.hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        // Windows keeps the menu on screen until something else happens; a
        // posted null message is the documented nudge.
        let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    }
}

/// A full screen app, or presentation mode: leave that screen alone.
fn fullscreen_busy() -> bool {
    unsafe {
        match SHQueryUserNotificationState() {
            Ok(state) => matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN),
            Err(_) => false,
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !pointer.is_null() {
        let app = &mut *pointer;
        match msg {
            WM_TIMER => {
                app.tick();
                return LRESULT(0);
            }
            WM_HOTKEY => {
                match wparam.0 as i32 {
                    HOTKEY_TOGGLE => app.toggle(),
                    HOTKEY_QUIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_COMMAND => {
                match wparam.0 & 0xFFFF {
                    MENU_TOGGLE => app.toggle(),
                    MENU_QUIT => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_TRAY => {
                let event = (lparam.0 & 0xFFFF) as u32;
                if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                    app.tray_menu();
                }
                return LRESULT(0);
            }
            WM_DESTROY => {
                app.remove_tray();
                PostQuitMessage(0);
                return LRESULT(0);
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() {
    unsafe {
        // Per monitor: the strip keeps the same thickness on a 150% screen.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let _singleton = CreateMutexW(None, true, w!("Local\\PiclineForWindows"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        let _gdiplus = GdiPlus::start();

        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PiclineLine");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE(instance.0),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class,
        };
        if RegisterClassW(&window_class) == 0 {
            return;
        }

        let style = WS_EX_LAYERED
            | WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_NOACTIVATE
            | WS_EX_TRANSPARENT;
        let Ok(hwnd) = CreateWindowExW(
            style,
            class,
            w!("Picline"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            HINSTANCE(instance.0),
            None,
        ) else {
            return;
        };

        let app = Box::into_raw(Box::new(App::new(hwnd)));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        (*app).add_tray();

        let _ = RegisterHotKey(hwnd, HOTKEY_TOGGLE, MOD_CONTROL | MOD_ALT, 0x54);
        let _ = RegisterHotKey(hwnd, HOTKEY_QUIT, MOD_CONTROL | MOD_ALT | MOD_SHIFT, 0x51);
        SetTimer(hwnd, TIMER_TICK, 16, None);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        drop(Box::from_raw(app));
    }
}
