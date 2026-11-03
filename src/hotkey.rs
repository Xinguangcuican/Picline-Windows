//! The global shortcut that shows and hides the line: Ctrl + Alt + T unless
//! changed, or none at all.
//!
//! Windows has no system-wide way to ask for a combination, so the app asks
//! for it in a small window of its own, the same gesture as the Mac build:
//! press the combination you want, Escape cancels, Delete removes it. The
//! window is a bare `WS_POPUP` with one label, because a dialog resource would
//! have to be compiled into the exe and this is one screen of text.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::SS_CENTER;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, SetFocus, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT,
    MOD_CONTROL, MOD_SHIFT, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_LWIN,
    VK_MENU, VK_RWIN, VK_SHIFT, VK_SPACE, VK_T,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, MessageBoxW, PostMessageW,
    RegisterClassW, SetForegroundWindow, ShowWindow, MB_ICONWARNING, MB_OK, SM_CXSCREEN,
    SM_CYSCREEN, SW_SHOW, WINDOW_STYLE, WM_APP, WNDCLASSW, WS_CAPTION, WS_CHILD,
    WS_EX_TOPMOST, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
};

use crate::settings;

/// The id the strip registers this shortcut under.
pub const ID: i32 = 1;
/// Posted to the strip when the recorder has finished; the result tells it
/// what happened.
pub const CHANGED: u32 = WM_APP + 3;

pub const RESULT_CANCELLED: usize = 0;
pub const RESULT_CLEARED: usize = 1;
pub const RESULT_PICKED: usize = 2;

/// A combination to hold down. `modifiers` keeps the Win32 `MOD_*` bits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HotKey {
    modifiers: u32,
    key: u32,
}

impl HotKey {
    pub const DEFAULT: HotKey = HotKey {
        modifiers: MOD_CONTROL.0 | MOD_ALT.0,
        key: VK_T.0 as u32,
    };

    /// "Ctrl + Alt + T", for the tray.
    pub fn label(self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.modifiers & MOD_CONTROL.0 != 0 {
            parts.push("Ctrl");
        }
        if self.modifiers & MOD_ALT.0 != 0 {
            parts.push("Alt");
        }
        if self.modifiers & MOD_SHIFT.0 != 0 {
            parts.push("Shift");
        }
        let name = key_name(self.key);
        parts.push(&name);
        parts.join(" + ")
    }

    /// "ctrl+alt+t": what goes in the settings file.
    fn code(self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.modifiers & MOD_CONTROL.0 != 0 {
            parts.push("ctrl".into());
        }
        if self.modifiers & MOD_ALT.0 != 0 {
            parts.push("alt".into());
        }
        if self.modifiers & MOD_SHIFT.0 != 0 {
            parts.push("shift".into());
        }
        parts.push(key_code(self.key));
        parts.join("+")
    }

    fn parse(text: &str) -> Option<HotKey> {
        let mut modifiers = 0u32;
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" => modifiers |= MOD_CONTROL.0,
                "alt" => modifiers |= MOD_ALT.0,
                "shift" => modifiers |= MOD_SHIFT.0,
                other => key = parse_key(other),
            }
        }
        let key = key?;
        // A bare key would fire while typing anywhere on the system.
        if modifiers == 0 {
            return None;
        }
        Some(HotKey { modifiers, key })
    }

    fn register(self, hwnd: HWND) -> bool {
        unsafe {
            RegisterHotKey(hwnd, ID, HOT_KEY_MODIFIERS(self.modifiers), self.key).is_ok()
        }
    }

    fn unregister(hwnd: HWND) {
        unsafe {
            let _ = UnregisterHotKey(hwnd, ID);
        }
    }
}

/// What the settings file remembers. `None` means the user removed it.
pub fn load() -> Option<HotKey> {
    match settings::get("hotkey") {
        None => Some(HotKey::DEFAULT),
        Some(text) if text == "none" => None,
        Some(text) => HotKey::parse(&text).or(Some(HotKey::DEFAULT)),
    }
}

/// Registers `want`, and says whether the system accepted it. A combination
/// another program already holds fails here, which is worth telling the user
/// about rather than quietly having no shortcut.
pub fn apply(hwnd: HWND, want: Option<HotKey>) -> bool {
    HotKey::unregister(hwnd);
    match want {
        None => {
            settings::set("hotkey", "none");
            true
        }
        Some(key) => {
            if key.register(hwnd) {
                settings::set("hotkey", &key.code());
                true
            } else {
                false
            }
        }
    }
}

/// Called when the system refused the combination: put back the one that
/// worked and say so.
pub fn complain(hwnd: HWND) {
    unsafe {
        let _ = MessageBoxW(
            hwnd,
            w!("这个组合被其他程序占用了，或者系统不允许。换个组合试试。"),
            w!("Picline"),
            MB_OK | MB_ICONWARNING,
        );
    }
}

// --- recording ------------------------------------------------------------

static OWNER: AtomicIsize = AtomicIsize::new(0);
static PICKED_MODIFIERS: AtomicU32 = AtomicU32::new(0);
static PICKED_KEY: AtomicU32 = AtomicU32::new(0);

/// The combination the recorder settled on. Only meaningful after a
/// `RESULT_PICKED`.
pub fn picked() -> HotKey {
    HotKey {
        modifiers: PICKED_MODIFIERS.load(Ordering::SeqCst),
        key: PICKED_KEY.load(Ordering::SeqCst),
    }
}

/// Opens the little window and returns at once: the result arrives at the
/// strip's window as `CHANGED`.
pub fn record(owner: HWND) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PiclineHotKey");
        // 6 is COLOR_WINDOW + 1: the system colour constant has to be offset
        // by one to be a brush, which is a Genuine Win32 Oddity.
        let definition = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(instance.0),
            // The recorder is the one window with a title bar, so it is the one
            // that shows the app icon in the corner and in Alt-Tab.
            hIcon: crate::app_icon(HINSTANCE(instance.0), false),
            lpszClassName: class,
            hbrBackground: HBRUSH(6 as *mut c_void),
            ..Default::default()
        };
        let _ = RegisterClassW(&definition);
        OWNER.store(owner.0 as isize, Ordering::SeqCst);

        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        let w = 460;
        let h = 132;
        let Ok(window) = CreateWindowExW(
            WS_EX_TOPMOST,
            class,
            w!("Picline 快捷键"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            (width - w) / 2,
            (height - h) / 3,
            w,
            h,
            None,
            None,
            instance,
            None,
        ) else {
            return;
        };
        let label = "按下新的快捷键\n\nEsc 取消　·　Delete 取消设置";
        let text = crate::wide(label);
        let child = CreateWindowExW(
            Default::default(),
            w!("STATIC"),
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_CENTER.0),
            14,
            16,
            w - 28,
            h - 48,
            window,
            None,
            instance,
            None,
        );
        let _ = child;
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(window);
        crate::clipboard::trace("hotkey recorder opened");
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{WM_CHAR, WM_CLOSE, WM_KEYDOWN, WM_SYSCHAR, WM_SYSKEYDOWN};
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            press(hwnd, wparam.0 as u16);
            LRESULT(0)
        }
        // Alt combinations would otherwise beep and reach the window menu.
        WM_SYSCHAR | WM_CHAR => LRESULT(0),
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// One key press inside the recorder.
fn press(hwnd: HWND, key: u16) {
    let held = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) < 0 };

    // Escape and Delete mean "no change" and "no shortcut", but only on their
    // own: Ctrl+Delete is a perfectly good combination to want.
    let bare = !held(VK_CONTROL) && !held(VK_MENU) && !held(VK_SHIFT);
    if bare && key == VK_ESCAPE.0 {
        finish(hwnd, RESULT_CANCELLED);
        return;
    }
    if bare && (key == VK_DELETE.0 || key == VK_BACK.0) {
        finish(hwnd, RESULT_CLEARED);
        return;
    }
    // The modifier itself: wait for the key that comes with it.
    if matches!(key, k if k == VK_CONTROL.0 || k == VK_SHIFT.0 || k == VK_MENU.0
        || k == VK_LWIN.0 || k == VK_RWIN.0)
    {
        return;
    }

    let mut modifiers = 0u32;
    if held(VK_CONTROL) {
        modifiers |= MOD_CONTROL.0;
    }
    if held(VK_MENU) {
        modifiers |= MOD_ALT.0;
    }
    if held(VK_SHIFT) {
        modifiers |= MOD_SHIFT.0;
    }
    // A key on its own would fire while typing in any program.
    if modifiers == 0 {
        return;
    }
    // Only keys with a name can be written to the settings file later.
    if key_code(key as u32).starts_with("vk") {
        return;
    }
    PICKED_MODIFIERS.store(modifiers, Ordering::SeqCst);
    PICKED_KEY.store(key as u32, Ordering::SeqCst);
    finish(hwnd, RESULT_PICKED);
}

fn finish(hwnd: HWND, result: usize) {
    let owner = OWNER.swap(0, Ordering::SeqCst);
    unsafe {
        let _ = DestroyWindow(hwnd);
        if owner != 0 {
            let _ = PostMessageW(HWND(owner as *mut c_void), CHANGED, WPARAM(result), LPARAM(0));
        }
    }
}

// --- names ----------------------------------------------------------------

fn key_name(key: u32) -> String {
    match key {
        k if (b'A' as u32..=b'Z' as u32).contains(&k) => ((k as u8) as char).to_string(),
        k if (b'0' as u32..=b'9' as u32).contains(&k) => ((k as u8) as char).to_string(),
        k if (0x70..=0x87).contains(&k) => format!("F{}", k - 0x6F),
        0x20 => "Space".to_string(),
        other => format!("键码 {other}"),
    }
}

fn key_code(key: u32) -> String {
    match key {
        k if (b'A' as u32..=b'Z' as u32).contains(&k) => ((k as u8) as char).to_ascii_lowercase().to_string(),
        k if (b'0' as u32..=b'9' as u32).contains(&k) => ((k as u8) as char).to_string(),
        k if (0x70..=0x87).contains(&k) => format!("f{}", k - 0x6F),
        k if k == VK_SPACE.0 as u32 => "space".to_string(),
        other => format!("vk{other}"),
    }
}

fn parse_key(text: &str) -> Option<u32> {
    let upper = text.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Some(bytes[0] as u32);
    }
    if upper == "SPACE" {
        return Some(VK_SPACE.0 as u32);
    }
    if let Some(number) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x6F + number);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_round_trips_through_the_settings_text() {
        let text = HotKey::DEFAULT.code();
        assert_eq!(text, "ctrl+alt+t");
        assert_eq!(HotKey::parse(&text), Some(HotKey::DEFAULT));
    }

    #[test]
    fn a_combination_needs_a_modifier() {
        assert_eq!(HotKey::parse("t"), None);
    }

    #[test]
    fn function_keys_and_space_have_names() {
        let f5 = HotKey { modifiers: MOD_CONTROL.0, key: 0x74 };
        assert_eq!(f5.code(), "ctrl+f5");
        assert_eq!(HotKey::parse("ctrl+f5"), Some(f5));
        assert_eq!(key_name(0x74), "F5");

        let space = HotKey { modifiers: MOD_ALT.0, key: VK_SPACE.0 as u32 };
        assert_eq!(space.code(), "alt+space");
        assert_eq!(HotKey::parse("alt+space"), Some(space));
    }

    #[test]
    fn a_label_reads_like_a_shortcut() {
        assert_eq!(HotKey::DEFAULT.label(), "Ctrl + Alt + T");
    }
}
