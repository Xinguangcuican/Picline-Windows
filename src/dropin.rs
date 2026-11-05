//! Taking pictures in by dragging them onto the line.
//!
//! The Mac build hands a dropped picture to `NSDraggingDestination`. On
//! Windows the same job is the shell's `WM_DROPFILES`: `DragAcceptFiles` tells
//! Explorer, the desktop, and every other program that this window takes
//! dropped files, and the message carries the list when the mouse is let go.
//!
//! That is the path the rest of the shell already understands, and it ends in
//! the same `add_files` call as "挂到 Picline", so a picture dragged in and a
//! picture sent from Explorer's menu look identical once they are cards.
//!
//! One catch: the strip is normally click-through, and the shell skips a
//! click-through window when looking for somewhere to drop. The app turns
//! that off as soon as a picture is carried over the top of the screen, which
//! is also when the line comes down to meet it; see `set_catching` in
//! `main.rs`.

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};

/// Tells the shell this window takes dropped files. Harmless to repeat.
pub fn accept(hwnd: HWND) {
    unsafe {
        DragAcceptFiles(hwnd, true);
    }
}

/// The file paths in a drop, in the order they were selected.
pub fn paths(hdrop: HDROP) -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        for index in 0..count {
            let len = DragQueryFileW(hdrop, index, None);
            if len == 0 {
                continue;
            }
            let mut buffer = vec![0u16; len as usize + 1];
            let got = DragQueryFileW(hdrop, index, Some(buffer.as_mut_slice()));
            if got > 0 {
                let name = std::ffi::OsString::from_wide(&buffer[..got as usize]);
                out.push(PathBuf::from(name));
            }
        }
    }
    out
}

/// Releases the drop handle. Must be called whatever the list turned out to
/// be, or the shell leaks it.
pub fn finish(hdrop: HDROP) {
    unsafe {
        DragFinish(hdrop);
    }
}
