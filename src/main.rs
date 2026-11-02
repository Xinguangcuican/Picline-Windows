//! Picline for Windows.
//!
//! Watch the pointer, and when it rests against the top edge of a screen, slide
//! a line down from behind the edge; move away and it goes back up. Pictures
//! that arrive on the clipboard — a screenshot, a copied file, a copied image —
//! hang on that line as cards, newest on the right.
//!
//! Keys: Ctrl+Alt+T brings it down or puts it away, Ctrl+Alt+Shift+Q quits.
//! The tray icon does the same two things with the mouse.
//!
//! Explorer can also hand pictures over: "挂到 Picline" in the right-click menu
//! starts this exe with `--hang` and the selected paths. If a copy is already
//! running, the paths are passed to it instead of starting a second one.
//!
//! A card can be clicked (preview), right-clicked (menu), or dragged out: the
//! picture is offered to Windows as a file, so it can be dropped into a chat
//! window, the desktop, or any folder.

#![windows_subsystem = "windows"]

mod canvas;
mod clipboard;
mod drag;
mod gdiplus;
mod hotkey;
mod settings;
mod preview;
mod shellmenu;

use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use canvas::Canvas;
use clipboard::Shot;
use gdiplus::{GdiPlus, Painter};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, ReleaseCapture, SetCapture, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
    VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN,
    SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::*;
/// How close to the top counts as touching it.
const BAND: i32 = 3;
/// Resting this long against the edge brings the line down. Long enough that a
/// quick trip to a maximised window's buttons does not.
const REVEAL_DELAY: Duration = Duration::from_millis(90);
/// Away this long and it goes back up.
const RETRACT_DELAY: Duration = Duration::from_millis(500);
/// After a picture arrives the line shows itself for this long, so you can see
/// that it took, and then puts itself away again.
const PEEK_FOR: Duration = Duration::from_millis(1600);
/// The clipboard is often still held by the app that just wrote it. Wait a
/// moment, then keep trying every tick for this many attempts.
const CLIPBOARD_SETTLE: Duration = Duration::from_millis(60);
const CLIPBOARD_TRIES: u32 = 24;
/// One slide, either way, in seconds.
const SLIDE: f32 = 0.20;
/// How far below the strip the pointer still counts as "on the line".
const ZONE_SLACK: i32 = 24;
/// Pictures kept in memory. The oldest drops off the left.
const MAX_SHOTS: usize = 12;

const PANEL_HEIGHT: f32 = 232.0;
const TIMER_TICK: usize = 1;
const HOTKEY_TOGGLE: i32 = 1;
const HOTKEY_QUIT: i32 = 2;
const TRAY_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const MENU_TOGGLE: usize = 1;
const MENU_CLEAR: usize = 2;
const MENU_QUIT: usize = 3;
const MENU_SHELL: usize = 4;
const MENU_REVEAL_NONE: usize = 10;
const MENU_REVEAL_CTRL: usize = 11;
const MENU_REVEAL_ALT: usize = 12;
const MENU_REVEAL_SHIFT: usize = 13;
const MENU_HOTKEY: usize = 5;
/// The per card menu. Numbers start well above the tray's so the two can
/// never be confused.
const CARD_PREVIEW: usize = 101;
const CARD_COPY: usize = 102;
const CARD_COPY_FILE: usize = 103;
const CARD_SAVE: usize = 104;
const CARD_OPEN: usize = 105;
const CARD_REVEAL: usize = 106;
const CARD_TAKE: usize = 111;
const CARD_LEFT: usize = 112;
const CARD_RIGHT: usize = 113;
const CARD_ALL: usize = 114;
/// Marks the `WM_COPYDATA` payload as a list of paths, so the receiving copy
/// knows what it was handed.
const COPYDATA_PATHS: usize = 0x5049434C; // "PICL"

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// A key held while the pointer rests against the top edge. `None` keeps the
/// old behaviour: the pointer alone brings the line down.
#[derive(Clone, Copy, PartialEq)]
enum RevealKey {
    None,
    Ctrl,
    Alt,
    Shift,
}

impl RevealKey {
    const ALL: [RevealKey; 4] = [RevealKey::None, RevealKey::Ctrl, RevealKey::Alt, RevealKey::Shift];

    fn code(self) -> &'static str {
        match self {
            RevealKey::None => "none",
            RevealKey::Ctrl => "ctrl",
            RevealKey::Alt => "alt",
            RevealKey::Shift => "shift",
        }
    }

    fn from_code(text: &str) -> RevealKey {
        match text {
            "ctrl" => RevealKey::Ctrl,
            "alt" => RevealKey::Alt,
            "shift" => RevealKey::Shift,
            _ => RevealKey::None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            RevealKey::None => "不需要按键",
            RevealKey::Ctrl => "按住 Ctrl",
            RevealKey::Alt => "按住 Alt",
            RevealKey::Shift => "按住 Shift",
        }
    }

    fn menu_id(self) -> usize {
        match self {
            RevealKey::None => MENU_REVEAL_NONE,
            RevealKey::Ctrl => MENU_REVEAL_CTRL,
            RevealKey::Alt => MENU_REVEAL_ALT,
            RevealKey::Shift => MENU_REVEAL_SHIFT,
        }
    }

    fn from_menu(id: usize) -> Option<RevealKey> {
        match id {
            MENU_REVEAL_NONE => Some(RevealKey::None),
            MENU_REVEAL_CTRL => Some(RevealKey::Ctrl),
            MENU_REVEAL_ALT => Some(RevealKey::Alt),
            MENU_REVEAL_SHIFT => Some(RevealKey::Shift),
            _ => None,
        }
    }

    /// Held right now, read straight from the keyboard state.
    fn held(self) -> bool {
        let key = match self {
            RevealKey::None => return true,
            RevealKey::Ctrl => VK_CONTROL,
            RevealKey::Alt => VK_MENU,
            RevealKey::Shift => VK_SHIFT,
        };
        unsafe { GetKeyState(key.0 as i32) < 0 }
    }

    fn load() -> RevealKey {
        settings::get("revealKey")
            .map(|text| RevealKey::from_code(&text))
            .unwrap_or(RevealKey::None)
    }

    fn save(self) {
        settings::set("revealKey", self.code());
    }
}

/// One card on the line, in logical pixels of the strip.
struct Card {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    shot: usize,
}

/// What the pointer is over: a card, or the cross that takes it down.
#[derive(Clone, Copy)]
struct Hit {
    shot: usize,
    close: bool,
}

/// Largest rectangle with that shape that fits in the box.
fn fit(aspect: f32, max_w: f32, max_h: f32) -> (f32, f32) {
    let aspect = aspect.max(0.05);
    let mut w = max_w;
    let mut h = w / aspect;
    if h > max_h {
        h = max_h;
        w = h * aspect;
    }
    (w.max(1.0), h.max(1.0))
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
    /// Opened on purpose with the key, the tray, or a new picture, so it stays
    /// while empty or while the pointer is away.
    pinned: bool,
    /// When the line should put itself away after showing a fresh picture.
    peek_until: Option<Instant>,
    shots: Vec<Shot>,
    /// A clipboard change waiting for the app that wrote it to let go.
    clip_pending: Option<(Instant, u32)>,
    /// Whether "挂到 Picline" is in Explorer's right-click menu right now.
    shell_menu: bool,
    /// What has to be held for the pointer against the top edge to count.
    reveal_key: RevealKey,
    last_tick: Instant,
    /// Where the strip was last put, so pointer positions can be turned into
    /// card positions.
    origin: (i32, i32),
    /// The cards as they were last drawn.
    cards: Vec<Card>,
    /// The card the pointer is over, by shot index.
    hover: Option<usize>,
    /// Whether the strip is currently catching the mouse. It only does so
    /// while a card is under the pointer, so the rope never eats a click meant
    /// for a maximised window's buttons.
    catching: bool,
    /// A card menu is open: no fading away underneath it.
    menu_open: bool,
    /// The big preview is up.
    preview_open: bool,
    /// Which card the open menu belongs to.
    menu_target: Option<usize>,
    /// A plain window used only to own popup menus; a window that refuses
    /// activation cannot dismiss its own menu.
    menu_owner: HWND,
    /// The left button went down on a card and has not come up yet.
    pressed: bool,
    /// Where that press happened, in screen pixels, so a small movement can be
    /// told from the start of a drag.
    press_from: Option<POINT>,
    /// The card under that press.
    press_shot: Option<usize>,
    /// Which cards are picked out, by picture fingerprint: one click picks one,
    /// shift takes the run from the last one picked, Ctrl adds or removes one.
    selected: Vec<u64>,
    /// Where a shift click measures from.
    anchor: Option<u64>,
    /// A press inside a group, held back until the button comes up so that a
    /// drag which follows can still carry the whole group.
    pending_collapse: Option<u64>,
    /// A drag is running: the strip holds still and lets the mouse through
    /// while the picture is carried somewhere else.
    dragging: bool,
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
            peek_until: None,
            shots: Vec::new(),
            clip_pending: None,
            shell_menu: false,
            reveal_key: RevealKey::load(),
            last_tick: Instant::now(),
            origin: (0, 0),
            cards: Vec::new(),
            hover: None,
            catching: false,
            menu_open: false,
            preview_open: false,
            menu_target: None,
            menu_owner: HWND::default(),
            pressed: false,
            press_from: None,
            press_shot: None,
            selected: Vec::new(),
            anchor: None,
            pending_collapse: None,
            dragging: false,
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

    /// Puts the cards in a row under the lowest part of the rope, newest on
    /// the right. Drops the oldest ones when they stop fitting.
    fn card_layout(&self, width: f32, middle: f32, scale: f32) -> Vec<Card> {
        let gap = 16.0 * scale;
        let max_w = 168.0 * scale;
        let max_h = 132.0 * scale;
        let usable = (width - 80.0 * scale).max(160.0 * scale);
        let clip = 18.0 * scale;

        let mut count = self.shots.len().min(MAX_SHOTS);
        loop {
            let start = self.shots.len() - count;
            let mut sizes: Vec<(f32, f32)> = self.shots[start..]
                .iter()
                .map(|s| fit(s.aspect(), max_w, max_h))
                .collect();
            let mut total: f32 =
                sizes.iter().map(|s| s.0).sum::<f32>() + gap * count.saturating_sub(1) as f32;
            let shrink = (usable / total).min(1.0);
            if shrink >= 0.62 || count == 1 {
                for size in sizes.iter_mut() {
                    size.0 *= shrink;
                    size.1 *= shrink;
                }
                total = sizes.iter().map(|s| s.0).sum::<f32>() + gap * count.saturating_sub(1) as f32;
                let mut x = (width - total) / 2.0;
                let y = middle + clip + 2.0 * scale;
                let mut cards = Vec::with_capacity(count);
                for (offset, (w, h)) in sizes.iter().enumerate() {
                    cards.push(Card {
                        x,
                        y,
                        w: *w,
                        h: *h,
                        shot: start + offset,
                    });
                    x += w + gap;
                }
                return cards;
            }
            count -= 1;
        }
    }

    fn paint(&mut self) {
        let mut laid_out: Vec<Card> = Vec::new();
        {
            let Some(canvas) = self.canvas.as_ref() else { return };
            canvas.clear();
            let width = canvas.width as f32;
            let scale = self.scale;
            let top = 10.0 * scale;
            let sag = (30.0 * scale).min(width * 0.018);
            // The lowest point of the rope, which is where things hang from.
            let middle = top + sag;

            let Some(painter) = Painter::new(canvas.hdc()) else { return };
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

            if self.shots.is_empty() {
                let hint = "截图、复制图片，或右键「挂到 Picline」";
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
            } else {
                let cards = self.card_layout(width, middle, scale);
                for card in &cards {
                    let shot = &self.shots[card.shot];
                    let center = card.x + card.w / 2.0;
                    // Where the rope actually is above this card: the curve
                    // sags most in the middle and hardly at all at the edges.
                    let t = (center / width).clamp(0.0, 1.0);
                    let rope_y = top + 4.0 * sag * t * (1.0 - t);

                    let radius = 8.0 * scale;
                    let clip_w = 18.0 * scale;
                    let clip_h = 16.0 * scale;

                    painter.line(
                        (center, rope_y),
                        (center, card.y - 1.0 * scale),
                        1.2 * scale,
                        argb(190, 130, 130, 130),
                    );
                    painter.rounded(
                        card.x + 1.0 * scale,
                        card.y + 3.0 * scale,
                        card.w,
                        card.h,
                        radius,
                        argb(48, 0, 0, 0),
                    );
                    painter.rounded(
                        card.x,
                        card.y,
                        card.w,
                        card.h,
                        radius,
                        argb(250, 251, 250, 247),
                    );

                    let pad = 5.0 * scale;
                    let (image_w, image_h) =
                        fit(shot.aspect(), card.w - pad * 2.0, card.h - pad * 2.0);
                    let image_x = card.x + (card.w - image_w) / 2.0;
                    let image_y = card.y + (card.h - image_h) / 2.0;
                    painter.image(
                        &shot.pixels,
                        shot.width,
                        shot.height,
                        image_x,
                        image_y,
                        image_w,
                        image_h,
                    );
                    painter.rounded_outline(
                        card.x,
                        card.y,
                        card.w,
                        card.h,
                        radius,
                        1.0 * scale,
                        argb(70, 0, 0, 0),
                    );

                    // The peg the card hangs by.
                    painter.rounded(
                        center - clip_w / 2.0,
                        card.y - clip_h,
                        clip_w,
                        clip_h + 4.0 * scale,
                        4.0 * scale,
                        argb(245, 92, 94, 98),
                    );
                    painter.rounded(
                        center - clip_w / 2.0 + 3.5 * scale,
                        card.y - clip_h + 3.0 * scale,
                        clip_w - 7.0 * scale,
                        3.5 * scale,
                        2.0 * scale,
                        argb(200, 196, 198, 202),
                    );

                    // Picked out: a ring just outside the card, so the picture
                    // itself is never covered.
                    if self.is_selected(card.shot) {
                        painter.rounded_outline(
                            card.x - 2.0 * scale,
                            card.y - 2.0 * scale,
                            card.w + 4.0 * scale,
                            card.h + 4.0 * scale,
                            radius + 2.0 * scale,
                            2.0 * scale,
                            argb(235, 74, 144, 255),
                        );
                    }

                    // Under the pointer: an accent edge and the cross that
                    // takes the picture down, the same way out as on the Mac.
                    if self.hover == Some(card.shot) {
                        painter.rounded_outline(
                            card.x,
                            card.y,
                            card.w,
                            card.h,
                            radius,
                            2.0 * scale,
                            argb(220, 74, 144, 255),
                        );
                        let r = 9.5 * scale;
                        let cross_x = card.x + card.w - 12.0 * scale;
                        let cross_y = card.y + 12.0 * scale;
                        painter.rounded(
                            cross_x - r,
                            cross_y - r,
                            r * 2.0,
                            r * 2.0,
                            r,
                            argb(238, 26, 26, 28),
                        );
                        painter.rounded_outline(
                            cross_x - r,
                            cross_y - r,
                            r * 2.0,
                            r * 2.0,
                            r,
                            1.0 * scale,
                            argb(90, 255, 255, 255),
                        );
                        let arm = 3.6 * scale;
                        let ink = argb(240, 245, 245, 245);
                        painter.line(
                            (cross_x - arm, cross_y - arm),
                            (cross_x + arm, cross_y + arm),
                            1.6 * scale,
                            ink,
                        );
                        painter.line(
                            (cross_x - arm, cross_y + arm),
                            (cross_x + arm, cross_y - arm),
                            1.6 * scale,
                            ink,
                        );
                    }
                }
                laid_out = cards;
            }
        }
        self.cards = laid_out;
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
        self.origin = (self.monitor.left, y);
    }

    /// Paints the strip again right now. Anything that changes what is on it
    /// has to call this: while the line is resting, nothing else asks for a
    /// new frame.
    fn refresh(&mut self) {
        self.prune_selection();
        self.painted = false;
        if self.shown {
            self.paint();
            self.slide();
        }
    }

    fn retract_to_zero(&mut self) {
        self.target = 0.0;
        self.pinned = false;
    }

    // --- the pointer ------------------------------------------------------

    /// Pointer position in strip coordinates, or nothing when there is no
    /// strip drawn to point at.
    fn pointer(&self) -> Option<(f32, f32)> {
        if self.cards.is_empty() {
            return None;
        }
        let mut point = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut point);
        }
        Some((
            (point.x - self.origin.0) as f32,
            (point.y - self.origin.1) as f32,
        ))
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        let reach = 11.0 * self.scale;
        for card in self.cards.iter().rev() {
            // The cross is only live on the card the pointer is already on, so
            // it never takes a click from a neighbour.
            if self.hover == Some(card.shot) {
                let cross_x = card.x + card.w - 12.0 * self.scale;
                let cross_y = card.y + 12.0 * self.scale;
                if (x - cross_x).abs() <= reach && (y - cross_y).abs() <= reach {
                    return Some(Hit {
                        shot: card.shot,
                        close: true,
                    });
                }
            }
            if x >= card.x && x <= card.x + card.w && y >= card.y && y <= card.y + card.h {
                return Some(Hit {
                    shot: card.shot,
                    close: false,
                });
            }
        }
        None
    }

    fn current_hit(&self) -> Option<Hit> {
        let (x, y) = self.pointer()?;
        self.hit_at(x, y)
    }

    // --- picking several out ----------------------------------------------

    fn is_selected(&self, index: usize) -> bool {
        self.shots
            .get(index)
            .map(|shot| self.selected.contains(&shot.hash))
            .unwrap_or(false)
    }

    fn index_of(&self, hash: u64) -> Option<usize> {
        self.shots.iter().position(|shot| shot.hash == hash)
    }

    /// The button went down on a card. Shift takes the run from the anchor of
    /// the last pick, Ctrl adds or removes this one, a plain press picks this
    /// one out — unless it is already part of a group, which stays as it is
    /// until the button comes up, so a drag can carry all of it.
    fn press_card(&mut self, index: usize, shift: bool, ctrl: bool) {
        let Some(shot) = self.shots.get(index) else { return };
        let hash = shot.hash;

        if shift {
            self.pending_collapse = None;
            if let Some(anchor) = self.anchor.and_then(|a| self.index_of(a)) {
                let (from, to) = if anchor <= index {
                    (anchor, index)
                } else {
                    (index, anchor)
                };
                self.selected = self.shots[from..=to].iter().map(|shot| shot.hash).collect();
            } else {
                self.selected = vec![hash];
                self.anchor = Some(hash);
            }
        } else if ctrl {
            self.pending_collapse = None;
            if let Some(at) = self.selected.iter().position(|&picked| picked == hash) {
                self.selected.remove(at);
            } else {
                self.selected.push(hash);
            }
            self.anchor = Some(hash);
        } else if self.selected.len() > 1 && self.selected.contains(&hash) {
            self.pending_collapse = Some(hash);
        } else {
            self.pending_collapse = None;
            self.selected = vec![hash];
            self.anchor = Some(hash);
        }
        self.refresh();
    }

    /// The button came up. Without a drag, a press inside a group closes the
    /// group down to that one card, the way Explorer does it.
    fn settle_selection(&mut self, dragged: bool) {
        let Some(pending) = self.pending_collapse.take() else {
            return;
        };
        if dragged {
            return;
        }
        if self.index_of(pending).is_some() {
            self.selected = vec![pending];
            self.anchor = Some(pending);
            self.refresh();
        }
    }

    /// What a drag or a menu command acts on: the whole group when this card
    /// is part of it, otherwise just this one. A plain click leaves exactly one
    /// card picked, so "this one" needs no special case.
    fn targets(&self, index: usize) -> Vec<usize> {
        if self.selected.len() > 1 && self.is_selected(index) {
            self.shots
                .iter()
                .enumerate()
                .filter(|(_, shot)| self.selected.contains(&shot.hash))
                .map(|(at, _)| at)
                .collect()
        } else {
            vec![index]
        }
    }

    /// Cards that went away leave the selection with them.
    fn prune_selection(&mut self) {
        if self.selected.is_empty() && self.anchor.is_none() {
            return;
        }
        let live: Vec<u64> = self.shots.iter().map(|shot| shot.hash).collect();
        self.selected.retain(|hash| live.contains(hash));
        if self.anchor.map(|a| !live.contains(&a)).unwrap_or(false) {
            self.anchor = None;
        }
        if self
            .pending_collapse
            .map(|p| !live.contains(&p))
            .unwrap_or(false)
        {
            self.pending_collapse = None;
        }
    }

    fn set_hover(&mut self, hover: Option<usize>) {
        // While a picture is being carried the strip must stay out of the way:
        // taking the mouse back here would put the line between the picture
        // and the window it is being dropped on.
        if self.dragging {
            return;
        }
        if self.hover == hover {
            return;
        }
        self.hover = hover;
        self.refresh();
        self.set_catching(hover.is_some());
    }

    /// Turns a press that has moved far enough into a drag of that card. The
    /// strip gives the mouse back first, so the picture can be dropped on
    /// whatever is behind the line instead of on the line itself.
    fn maybe_drag(&mut self) {
        let (Some(index), Some(from)) = (self.press_shot, self.press_from) else {
            return;
        };
        let mut now = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut now);
        }
        let slop = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(4);
        if (now.x - from.x).abs() < slop && (now.y - from.y).abs() < slop {
            return;
        }

        // Worked out before anything moves: the group is what is picked out.
        let indices = self.targets(index);
        self.pending_collapse = None;
        self.pressed = false;
        self.press_shot = None;
        self.press_from = None;
        self.hover = None;
        unsafe {
            let _ = ReleaseCapture();
        }
        // Click-through while carrying: the line must not sit between the
        // picture and the window it is being dropped into.
        self.set_catching(false);
        self.refresh();

        let mut paths: Vec<PathBuf> = Vec::new();
        for at in &indices {
            match self.shots.get(*at).and_then(clipboard::ensure_drag_file) {
                Some(path) => paths.push(path),
                None => clipboard::trace(&format!("drag: no file for shot {at}")),
            }
        }
        if paths.is_empty() {
            clipboard::trace("drag: nothing to carry");
            return;
        }

        clipboard::trace(&format!("drag of {} picture(s): {:?}", paths.len(), paths));
        self.dragging = true;
        let note = drag::carry(&paths);
        self.dragging = false;
        clipboard::trace(&format!("drag finished: {note}"));

        self.set_hover(None);
        self.refresh();
    }

    /// A strip that is always in the way would be a nuisance: it only catches
    /// the mouse while one of its cards is under the pointer. Everything else
    /// goes through to whatever is behind it.
    fn set_catching(&mut self, on: bool) {
        if self.catching == on {
            return;
        }
        self.catching = on;
        let through = WS_EX_TRANSPARENT.0 as isize;
        let next;
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            next = if on { style & !through } else { style | through };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, next);
            // The new style has to be pushed through for hit testing to see
            // it straight away.
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        // A style change can make the layered surface stale; hand it over once
        // more so the strip never blinks out.
        if self.shown {
            self.slide();
        }
        // Worth a line in the log if the style did not take: then nothing on
        // the line could ever be clicked.
        let after = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) };
        if (after & through) != (next & through) {
            clipboard::trace(&format!("ex style did not take: {after} wanted {next}"));
        }
    }

    // --- what a card can do -----------------------------------------------

    fn preview(&mut self, index: usize) {
        let Some(shot) = self.shots.get(index) else {
            return;
        };
        let opened = preview::open(self.hwnd, shot, self.monitor);
        clipboard::trace(&format!("preview of shot {index}: {opened}"));
        if opened {
            self.preview_open = true;
        }
    }

    fn preview_closed(&mut self) {
        self.preview_open = false;
        if self.shown {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }

    fn copy_image(&mut self, index: usize) {
        let Some(shot) = self.shots.get(index) else {
            return;
        };
        let done = clipboard::put_image(shot);
        clipboard::trace(&format!("copy picture: {done}"));
    }

    /// Every picked picture, in one file list: that is how Windows carries a
    /// handful of files on the clipboard, and how a chat window sends several.
    fn copy_file(&mut self, index: usize) {
        let mut paths: Vec<PathBuf> = Vec::new();
        for at in self.targets(index) {
            match self.shots.get(at).and_then(clipboard::ensure_file) {
                Some(path) => paths.push(path),
                None => clipboard::trace(&format!("copy as file: nothing for shot {at}")),
            }
        }
        if paths.is_empty() {
            clipboard::trace("copy as file: nothing to copy");
            return;
        }
        clipboard::trace(&format!("copy as file: {} picture(s)", paths.len()));
        clipboard::put_files(&paths);
    }

    fn save_copy(&mut self, index: usize) {
        let group = self.targets(index);
        let mut saved = 0;
        for at in &group {
            match self.shots.get(*at).and_then(clipboard::save_to_desktop) {
                Some(path) => {
                    saved += 1;
                    clipboard::trace(&format!("saved a copy: {}", path.display()));
                }
                None => clipboard::trace("could not save a copy"),
            }
        }
        clipboard::trace(&format!("saved {saved} of {} picture(s)", group.len()));
    }

    fn open_with_viewer(&mut self, index: usize) {
        // The whole group opens; the viewer decides whether that means one
        // window per picture or one window with several.
        for at in self.targets(index) {
            let Some(path) = self.shots.get(at).and_then(clipboard::ensure_file) else {
                continue;
            };
            let file: Vec<u16> =
                path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            unsafe {
                ShellExecuteW(
                    None,
                    w!("open"),
                    PCWSTR(file.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
        }
    }

    fn reveal(&mut self, index: usize) {
        let Some(shot) = self.shots.get(index) else {
            return;
        };
        let Some(path) = clipboard::ensure_file(shot) else {
            return;
        };
        let mut arguments: Vec<u16> = format!("/select,\"{}\"", path.display())
            .encode_utf16()
            .collect();
        arguments.push(0);
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                w!("explorer.exe"),
                PCWSTR(arguments.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn forget(&mut self, index: usize) {
        if index >= self.shots.len() {
            return;
        }
        self.shots.remove(index);
        self.hover = None;
        self.update_tip();
        if self.shots.is_empty() {
            self.peek_until = None;
            self.retract_to_zero();
        }
        self.refresh();
    }

    /// The menu's "取下这张": the whole picked group when the card is part of
    /// it. Taken from the right first, so the indices to the left stay valid.
    fn forget_group(&mut self, index: usize) {
        let mut group = self.targets(index);
        group.sort_unstable();
        for at in group.into_iter().rev() {
            self.forget(at);
        }
    }

    /// Takes down everything on one side of a card, leaving the card itself.
    fn drop_side(&mut self, index: usize, to_the_left: bool) {
        if to_the_left {
            if index == 0 {
                return;
            }
            self.shots.drain(0..index);
        } else {
            self.shots.truncate(index + 1);
        }
        self.hover = None;
        self.update_tip();
        self.refresh();
    }

    // --- menus ------------------------------------------------------------

    /// Runs a popup menu and returns the item picked, or 0. A window that
    /// refuses activation cannot dismiss its own menu, so the menu is owned by
    /// a plain hidden window, and focus goes back where it was afterwards.
    unsafe fn run_menu(&self, menu: HMENU) -> u32 {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let previous = GetForegroundWindow();
        let _ = SetForegroundWindow(self.menu_owner);
        let picked = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            self.menu_owner,
            None,
        );
        if !previous.is_invalid() {
            let _ = SetForegroundWindow(previous);
        }
        let _ = PostMessageW(self.menu_owner, WM_NULL, WPARAM(0), LPARAM(0));
        picked.0 as u32
    }

    fn card_menu(&mut self, hit: Hit) {
        let Some(shot) = self.shots.get(hit.shot) else {
            return;
        };
        let has_file = shot.path.is_some();
        // Everything in the menu acts on what is picked out: three cards
        // picked and one of them right-clicked means three of everything.
        let group = self.targets(hit.shot);
        let picked = group.len();
        // "Everything to the left" only means something for one card: with
        // three picked out there is no single place to measure from.
        let left = picked == 1 && hit.shot > 0;
        let right = picked == 1 && hit.shot + 1 < self.shots.len();

        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        let flag = |on: bool| {
            if on {
                MF_STRING
            } else {
                MF_STRING | MF_GRAYED
            }
        };
        // "存到桌面（3 张）" while a group is picked out, plain otherwise.
        let name = |base: &str| -> Vec<u16> {
            let text = if picked > 1 {
                format!("{base}（{picked} 张）")
            } else {
                base.to_string()
            };
            text.encode_utf16().chain(std::iter::once(0)).collect()
        };
        unsafe {
            let _ = AppendMenuW(menu, MF_STRING, CARD_PREVIEW, w!("预览"));
            // A clipboard holds one bitmap, so this one stays on the card that
            // was right-clicked; files and the drag carry the whole group.
            let _ = AppendMenuW(menu, MF_STRING, CARD_COPY, w!("复制图片"));
            let label = name("复制为文件");
            let _ = AppendMenuW(menu, MF_STRING, CARD_COPY_FILE, PCWSTR(label.as_ptr()));
            let label = name("存到桌面");
            let _ = AppendMenuW(menu, MF_STRING, CARD_SAVE, PCWSTR(label.as_ptr()));
            let _ = AppendMenuW(
                menu,
                flag(has_file),
                CARD_REVEAL,
                w!("在资源管理器中显示"),
            );
            let label = name("用默认看图程序打开");
            let _ = AppendMenuW(menu, MF_STRING, CARD_OPEN, PCWSTR(label.as_ptr()));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let label = name("取下这张");
            let _ = AppendMenuW(menu, MF_STRING, CARD_TAKE, PCWSTR(label.as_ptr()));
            let _ = AppendMenuW(menu, flag(left), CARD_LEFT, w!("取下左边全部"));
            let _ = AppendMenuW(menu, flag(right), CARD_RIGHT, w!("取下右边全部"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CARD_ALL, w!("全部取下"));
        }

        self.menu_target = Some(hit.shot);
        self.menu_open = true;
        let picked = unsafe { self.run_menu(menu) };
        self.menu_open = false;
        unsafe {
            let _ = DestroyMenu(menu);
        }
        clipboard::trace(&format!("menu picked {picked}"));
        if picked != 0 {
            self.card_command(picked as usize);
        }
        self.menu_target = None;
    }

    fn card_command(&mut self, command: usize) {
        let Some(index) = self.menu_target else {
            return;
        };
        match command {
            CARD_PREVIEW => self.preview(index),
            CARD_COPY => self.copy_image(index),
            CARD_COPY_FILE => self.copy_file(index),
            CARD_SAVE => self.save_copy(index),
            CARD_REVEAL => self.reveal(index),
            CARD_OPEN => self.open_with_viewer(index),
            CARD_TAKE => self.forget_group(index),
            CARD_LEFT => self.drop_side(index, true),
            CARD_RIGHT => self.drop_side(index, false),
            CARD_ALL => self.take_everything_down(),
            _ => {}
        }
    }

    fn monitor_under_pointer() -> RECT {
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
        info.rcMonitor
    }

    /// A picture just arrived: show it, then get out of the way again.
    fn show_what_arrived(&mut self) {
        self.painted = false;
        if fullscreen_busy() {
            // Someone is watching a film or playing a game; no fanfare.
            return;
        }
        if self.target <= 0.0 {
            self.prepare(Self::monitor_under_pointer());
            self.target = 1.0;
            self.hot_since = None;
        }
        self.away_since = None;
        self.pinned = true;
        self.peek_until = Some(Instant::now() + PEEK_FOR);
        self.slide();
    }

    fn take_everything_down(&mut self) {
        self.shots.clear();
        self.painted = false;
        self.peek_until = None;
        self.hover = None;
        self.set_catching(false);
        self.retract_to_zero();
        self.update_tip();
    }

    /// Adds or removes the Explorer item, and remembers which it is now.
    fn toggle_shell_menu(&mut self) {
        if self.shell_menu {
            shellmenu::remove();
        } else {
            shellmenu::install();
        }
        self.shell_menu = shellmenu::installed();
        shellmenu::remember(self.shell_menu);
        clipboard::trace(&format!("Explorer item on: {}", self.shell_menu));
    }

    fn update_tip(&self) {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_TIP,
            ..Default::default()
        };
        let text = if self.shots.is_empty() {
            "Picline".to_string()
        } else {
            format!("Picline · 挂着 {} 张", self.shots.len())
        };
        let tip: Vec<u16> = text.encode_utf16().take(127).collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// Reads what the clipboard has, waits for it to be readable, and keeps
    /// only pictures that are not already on the line.
    fn check_clipboard(&mut self, now: Instant) {
        let Some((since, attempts)) = self.clip_pending else { return };
        if now.duration_since(since) < CLIPBOARD_SETTLE {
            return;
        }
        match clipboard::read(self.hwnd.0 as isize) {
            Some(shots) => {
                self.clip_pending = None;
                let mut arrived = false;
                for shot in shots {
                    if self.shots.iter().any(|old| old.hash == shot.hash) {
                        continue;
                    }
                    self.shots.push(shot);
                    arrived = true;
                }
                if arrived {
                    self.accepted();
                }
            }
            None => {
                // Still held by whoever wrote it. Keep trying, quietly.
                if attempts >= CLIPBOARD_TRIES {
                    self.clip_pending = None;
                    clipboard::trace("clipboard stayed busy; gave up this time");
                } else {
                    self.clip_pending = Some((since, attempts + 1));
                }
            }
        }
    }

    /// Puts pictures from files on disk onto the line. This is the route
    /// Explorer's "挂到 Picline" takes, and the same one a copied file takes;
    /// both end up looking identical once they are cards.
    fn add_files(&mut self, paths: &[PathBuf]) {
        let mut arrived = false;
        for path in paths {
            match clipboard::shot_from_path(path) {
                Some(shot) => {
                    if self.shots.iter().any(|old| old.hash == shot.hash) {
                        continue;
                    }
                    self.shots.push(shot);
                    arrived = true;
                }
                None => clipboard::trace(&format!("could not read {}", path.display())),
            }
        }
        if arrived {
            self.accepted();
        }
    }

    /// Shared tail end of anything that brings pictures in: cap the row,
    /// refresh the tray text, and show the line for a moment.
    fn accepted(&mut self) {
        while self.shots.len() > MAX_SHOTS {
            self.shots.remove(0);
        }
        self.update_tip();
        self.show_what_arrived();
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().clamp(0.0, 0.1);
        self.last_tick = now;

        self.check_clipboard(now);

        // A menu, a preview or a drag is in progress: the line holds still
        // underneath it.
        if self.menu_open || self.preview_open || self.dragging {
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
            if against_edge && !fullscreen_busy() && self.reveal_key.held() {
                let since = *self.hot_since.get_or_insert(now);
                if now.duration_since(since) >= REVEAL_DELAY {
                    self.hot_since = None;
                    self.prepare(rc);
                    self.target = 1.0;
                    self.away_since = None;
                    self.pinned = false;
                    self.peek_until = None;
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

        // The look-at-it pause after a picture arrives.
        if let Some(until) = self.peek_until {
            if now >= until && !on_the_line {
                self.peek_until = None;
                self.retract_to_zero();
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
            // for a strip nobody is looking at. The pictures stay.
            self.canvas = None;
            self.painted = false;
            self.cards.clear();
            self.hover = None;
            self.set_catching(false);
        }

        // Whatever the pointer is over decides whether the strip catches the
        // mouse at all, and which card shows its cross.
        let hit = if self.shown { self.current_hit() } else { None };
        self.set_hover(hit.map(|h| h.shot));
    }

    /// The recorder has finished. Apply what it chose; if the system refuses
    /// the combination because something else holds it, put the previous one
    /// back and say so instead of leaving the app without a shortcut.
    fn set_hotkey(&mut self, want: Option<hotkey::HotKey>) {
        let previous = hotkey::load();
        if hotkey::apply(self.hwnd, want) {
            let text = want.map(|key| key.label()).unwrap_or_else(|| "none".to_string());
            clipboard::trace(&format!("hotkey: {text}"));
        } else {
            hotkey::apply(self.hwnd, previous);
            hotkey::complain(self.hwnd);
        }
    }

    fn toggle(&mut self) {
        if self.target > 0.0 {
            self.peek_until = None;
            self.retract_to_zero();
            return;
        }
        self.prepare(Self::monitor_under_pointer());
        self.target = 1.0;
        self.pinned = true;
        self.away_since = None;
        self.peek_until = None;
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

    unsafe fn tray_menu(&mut self) {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_TOGGLE, w!("放下来 / 收起来"));
        // What has to be held for the pointer against the top edge to count.
        // A submenu of four radio items rather than a checkbox list, so the
        // tray stays the height it was.
        if let Ok(reveal) = CreatePopupMenu() {
            for key in RevealKey::ALL {
                let state = if self.reveal_key == key {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                };
                let text = wide(key.title());
                let _ = AppendMenuW(reveal, state, key.menu_id(), PCWSTR(text.as_ptr()));
            }
            let label = wide("贴顶唤出");
            let _ = AppendMenuW(menu, MF_POPUP, reveal.0 as usize, PCWSTR(label.as_ptr()));
        }
        let clear = if self.shots.is_empty() {
            MF_STRING | MF_GRAYED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(menu, clear, MENU_CLEAR, w!("全部取下"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let shell = if self.shell_menu {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(menu, shell, MENU_SHELL, w!("资源管理器右键「挂到 Picline」"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let shortcut = match hotkey::load() {
            Some(key) => format!("快捷键：{}…", key.label()),
            None => "快捷键：未设置…".to_string(),
        };
        let shortcut_label = wide(&shortcut);
        let _ = AppendMenuW(menu, MF_STRING, MENU_HOTKEY, PCWSTR(shortcut_label.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("退出"));
        let picked = self.run_menu(menu);
        let _ = DestroyMenu(menu);
        if let Some(key) = RevealKey::from_menu(picked as usize) {
            self.reveal_key = key;
            key.save();
            clipboard::trace(&format!("reveal key: {}", key.code()));
            return;
        }
        match picked as usize {
            MENU_TOGGLE => self.toggle(),
            MENU_CLEAR => self.take_everything_down(),
            MENU_SHELL => self.toggle_shell_menu(),
            MENU_HOTKEY => hotkey::record(self.hwnd),
            MENU_QUIT => {
                let _ = DestroyWindow(self.hwnd);
            }
            _ => {}
        }
    }
}

/// The default window procedure, as a plain function so it can be handed to
/// `WNDCLASSW` without the generics of the crate's wrapper.
unsafe extern "system" fn plain_window(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// An invisible, ordinary window that exists only to own popup menus. The
/// strip itself is `WS_EX_NOACTIVATE`, and a window that refuses activation
/// cannot dismiss its own menu when you click elsewhere.
fn menu_owner(instance: HINSTANCE) -> HWND {
    let class = w!("PiclineMenu");
    let definition = WNDCLASSW {
        lpfnWndProc: Some(plain_window),
        hInstance: instance,
        lpszClassName: class,
        ..Default::default()
    };
    unsafe {
        let _ = RegisterClassW(&definition);
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            w!("Picline menu"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        )
        .unwrap_or_default()
    }
}

/// A NUL-terminated UTF-16 copy, for the Win32 calls that take a pointer.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A full screen app, or presentation mode: leave that screen alone.
fn fullscreen_busy() -> bool {    unsafe {
        match SHQueryUserNotificationState() {
            Ok(state) => matches!(state, QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN),
            Err(_) => false,
        }
    }
}

/// The picture files Explorer put on the command line. Anything that is not a
/// file — the `--hang` flag, a `%1` the shell failed to expand — is logged and
/// dropped, so a launch never hangs an empty card.
fn hang_request() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let path = PathBuf::from(&arg);
        if path.is_file() {
            paths.push(path);
        } else if !arg.to_string_lossy().starts_with("--") {
            clipboard::trace(&format!("not a file: {}", path.display()));
        }
    }
    paths
}

/// Splits the double-NUL terminated list back into paths.
fn paths_from_blob(text: &[u16]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut start = 0;
    for (at, unit) in text.iter().enumerate() {
        if *unit != 0 {
            continue;
        }
        if at > start {
            let name = std::ffi::OsString::from_wide(&text[start..at]);
            paths.push(PathBuf::from(name));
        }
        start = at + 1;
    }
    paths
}

/// Hands the paths to the copy that is already running. Explorer runs the item
/// as a fresh process every time; this is what makes the second, third and
/// tenth picture land on the same line instead of starting a second rope.
unsafe fn forward(paths: &[PathBuf]) {
    let Ok(window) = FindWindowW(w!("PiclineLine"), w!("Picline")) else {
        clipboard::trace("no running copy to hand the pictures to");
        return;
    };
    let mut blob: Vec<u16> = Vec::new();
    for path in paths {
        blob.extend(path.as_os_str().encode_wide());
        blob.push(0);
    }
    blob.push(0);
    let data = COPYDATASTRUCT {
        dwData: COPYDATA_PATHS,
        cbData: blob.len() as u32 * 2,
        lpData: blob.as_ptr() as *mut _,
    };
    SendMessageW(
        window,
        WM_COPYDATA,
        WPARAM(window.0 as usize),
        LPARAM(&data as *const COPYDATASTRUCT as isize),
    );
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
            WM_CLIPBOARDUPDATE => {
                // Noting it now and reading it a moment later avoids the
                // classic race with the app that has just locked the board.
                app.clip_pending = Some((Instant::now(), 0));
                return LRESULT(0);
            }
            WM_MOUSEACTIVATE => {
                // Clicking a picture must not take the keyboard away from
                // whatever you were typing in.
                return LRESULT(MA_NOACTIVATE as isize);
            }
            WM_MOUSEMOVE => {
                // Only arrives while a card is under the pointer; keeps the
                // highlight in step without waiting for the timer.
                if app.pressed {
                    app.maybe_drag();
                }
                let hit = app.current_hit();
                app.set_hover(hit.map(|h| h.shot));
                return LRESULT(0);
            }
            WM_SETCURSOR => {
                // A hand over the cross, the four arrows over the picture: it
                // can be carried anywhere a file can be dropped.
                let shape = match app.current_hit() {
                    Some(hit) if hit.close => IDC_HAND,
                    Some(_) => IDC_SIZEALL,
                    None => IDC_ARROW,
                };
                unsafe {
                    let cursor = LoadCursorW(None, shape).unwrap_or_default();
                    let _ = SetCursor(cursor);
                }
                return LRESULT(1);
            }
            WM_LBUTTONDOWN => {
                match app.current_hit() {
                    Some(hit) if hit.close => {
                        clipboard::trace(&format!("cross clicked on shot {}", hit.shot));
                        app.forget(hit.shot)
                    }
                    Some(hit) => {
                        // Picking happens on the way down, so a drag that
                        // follows already knows what it is carrying.
                        app.press_card(
                            hit.shot,
                            unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 },
                            unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 },
                        );
                        app.pressed = true;
                        app.press_shot = Some(hit.shot);
                        let mut from = POINT::default();
                        unsafe {
                            let _ = GetCursorPos(&mut from);
                        }
                        app.press_from = Some(from);
                        unsafe {
                            SetCapture(hwnd);
                        }
                    }
                    None => {}
                }
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                if app.pressed {
                    app.pressed = false;
                    app.press_shot = None;
                    app.press_from = None;
                    // Letting go only settles the pick; the picture opens on a
                    // double click, as it does on the Mac.
                    app.settle_selection(false);
                }
                return LRESULT(0);
            }
            WM_LBUTTONDBLCLK => {
                if let Some(hit) = app.current_hit() {
                    if !hit.close {
                        app.preview(hit.shot);
                    }
                }
                return LRESULT(0);
            }
            WM_RBUTTONUP => {
                if let Some(hit) = app.current_hit() {
                    clipboard::trace(&format!("menu asked for shot {}", hit.shot));
                    app.card_menu(hit);
                }
                return LRESULT(0);
            }
            preview::CLOSED => {
                app.preview_closed();
                return LRESULT(0);
            }
            WM_COPYDATA => {
                let copy = &*(lparam.0 as *const COPYDATASTRUCT);
                if copy.dwData == COPYDATA_PATHS && !copy.lpData.is_null() && copy.cbData >= 2 {
                    let units = copy.cbData as usize / 2;
                    let blob = std::slice::from_raw_parts(copy.lpData as *const u16, units);
                    let paths = paths_from_blob(blob);
                    clipboard::trace(&format!("Explorer handed over {} picture(s)", paths.len()));
                    app.add_files(&paths);
                }
                return LRESULT(1);
            }
            hotkey::CHANGED => {
                match wparam.0 {
                    hotkey::RESULT_PICKED => app.set_hotkey(Some(hotkey::picked())),
                    hotkey::RESULT_CLEARED => app.set_hotkey(None),
                    _ => clipboard::trace("hotkey recording cancelled"),
                }
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
                if let Some(key) = RevealKey::from_menu(wparam.0 & 0xFFFF) {
                    app.reveal_key = key;
                    key.save();
                    return LRESULT(0);
                }
                match wparam.0 & 0xFFFF {
                    MENU_TOGGLE => app.toggle(),
                    MENU_CLEAR => app.take_everything_down(),
                    MENU_SHELL => app.toggle_shell_menu(),
                    MENU_HOTKEY => hotkey::record(hwnd),
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
        // OLE, for dragging pictures out of the line.
        drag::prepare();

        let requested = hang_request();

        let _singleton = CreateMutexW(None, true, w!("Local\\PiclineForWindows"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Explorer starts a fresh process for every click. Pass the
            // pictures to the copy that is already up and leave quietly.
            if !requested.is_empty() {
                forward(&requested);
            }
            return;
        }

        clipboard::trace("--- Picline for Windows started ---");

        let _gdiplus = GdiPlus::start();

        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PiclineLine");
        let window_class = WNDCLASSW {
            // Double clicks: a card opens on one, and Windows only sends
            // `WM_LBUTTONDBLCLK` to a class that asks for them.
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
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
        (*app).menu_owner = menu_owner(HINSTANCE(instance.0));
        (*app).add_tray();

        // Put "挂到 Picline" in Explorer's right-click menu. Rewritten on every
        // start so it always points at wherever the exe lives now, and it stays
        // gone once it has been switched off from the tray.
        if shellmenu::wanted() {
            shellmenu::install();
            (*app).shell_menu = shellmenu::installed();
        }
        clipboard::trace(&format!("Explorer item on: {}", (*app).shell_menu));

        if !requested.is_empty() {
            (*app).add_files(&requested);
        }

        clipboard::watch(hwnd.0 as isize);

        // The toggle combination is the user's to choose; the quit one is not,
        // because it is the way out when something has gone wrong.
        let want = hotkey::load();
        if !hotkey::apply(hwnd, want) {
            clipboard::trace("hotkey refused at startup, falling back to the default");
            hotkey::apply(hwnd, Some(hotkey::HotKey::DEFAULT));
        }
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
