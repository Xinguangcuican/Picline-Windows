//! Where pictures are written.
//!
//! Screenshots only exist in memory until something needs a file: pasting one
//! as a file, dragging a card out, showing it in Explorer. Those files used to
//! go to `%TEMP%\Picline`, which Windows is free to sweep up. The user can
//! point the folder at something they own — a work folder, a drive that gets
//! backed up — and the temp folder stays as the fallback for a drive that is
//! not plugged in, so a picture always has somewhere to land.

use std::path::{Path, PathBuf};

/// `%TEMP%\Picline`, where earlier versions put everything.
pub fn default_folder() -> PathBuf {
    std::env::temp_dir().join("Picline")
}

/// The user's choice, if it is still there. An unplugged drive answers None,
/// which is what sends the next picture back to the default folder.
pub fn chosen() -> Option<PathBuf> {
    let raw = crate::settings::get("folder")?;
    let path = PathBuf::from(raw);
    path.is_dir().then_some(path)
}

/// The folder in use right now.
pub fn folder() -> PathBuf {
    chosen().unwrap_or_else(default_folder)
}

/// Records the choice, or clears it when `path` is None. The folder is made
/// first, so picking a name that does not exist yet still works.
pub fn remember(path: Option<&Path>) {
    match path {
        Some(path) => {
            let _ = std::fs::create_dir_all(path);
            crate::settings::set("folder", &path.display().to_string());
        }
        None => crate::settings::remove("folder"),
    }
}

/// Whether a picture can be written there. A read-only drive, or a folder on a
/// disc that has been ejected, answers no.
pub fn is_usable(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let probe = path.join(".picline-write-test");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
