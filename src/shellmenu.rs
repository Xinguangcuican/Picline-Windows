//! The "挂到 Picline" item in Explorer's right-click menu.
//!
//! Windows offers two ways to get there: a COM shell extension — a DLL that has
//! to be built for both architectures, registered, kept in step with the shell,
//! and signed before most machines will load it — or a plain registry verb,
//! which is nothing more than a command line with a `%1` in it. A verb is all
//! this needs: it launches this same exe with `--hang` and the paths Explorer
//! passes, and the running copy takes it from there.
//!
//! Everything lives under `HKCU\Software\Classes`, so it applies to the person
//! who ran the app, needs no administrator, and comes off with one key delete.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

/// What the item says. Windows treats `&` as the mnemonic marker, so there is
/// deliberately none here — Chinese menus rarely use them.
const LABEL: &str = "挂到 Picline";

/// The file kinds the item is offered on. `image` is the perceived type Windows
/// already assigns to png, jpeg, gif, bmp, tiff and ico, which is exactly the
/// set GDI+ can decode. Naming extensions one by one on top of it risks the
/// shell showing the item twice for the same file, so it is left at one.
const KINDS: [&str; 1] = ["image"];

fn key_path(kind: &str) -> String {
    format!("Software\\Classes\\SystemFileAssociations\\{kind}\\shell\\Picline")
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Writes the item for every kind. Returns true only if all of them took.
pub fn install() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let exe = exe.display().to_string();
    // `%*` is every selected file, `%1` is the first one. Both are written on
    // purpose: a shell that expands only one of them still ends up correct, and
    // a picture that arrives twice is dropped by its fingerprint.
    let command = format!("\"{exe}\" --hang %* \"%1\"");
    let icon = format!("\"{exe}\",0");

    let mut all = true;
    for kind in KINDS {
        all &= write_verb(&key_path(kind), &command, &icon);
    }
    all
}

fn write_verb(path: &str, command: &str, icon: &str) -> bool {
    unsafe {
        let Some(key) = create(path) else { return false };
        let mut ok = set(key, None, LABEL);
        ok &= set(key, Some("Icon"), icon);
        // Without this the item disappears the moment more than one file is
        // selected. "Player" means the command is run once, with every selected
        // picture appended to the command line.
        ok &= set(key, Some("MultiSelectModel"), "Player");
        let _ = RegCloseKey(key);

        let command_key = format!("{path}\\command");
        let Some(key) = create(&command_key) else { return false };
        ok &= set(key, None, command);
        let _ = RegCloseKey(key);
        ok
    }
}

unsafe fn create(path: &str) -> Option<HKEY> {
    let name = wide(path);
    let mut key = HKEY::default();
    let status = RegCreateKeyExW(
        HKEY_CURRENT_USER,
        PCWSTR(name.as_ptr()),
        0,
        PCWSTR::null(),
        REG_OPTION_NON_VOLATILE,
        KEY_WRITE,
        None,
        &mut key,
        None,
    );
    if status.0 == 0 {
        Some(key)
    } else {
        None
    }
}

/// One string value, as UTF-16 the way the registry stores `REG_SZ`. The
/// trailing NUL is part of the value: without it the shell reads whatever
/// follows the bytes, and the menu item comes out with junk on the end.
unsafe fn set(key: HKEY, name: Option<&str>, value: &str) -> bool {
    let mut data: Vec<u16> = value.encode_utf16().collect();
    data.push(0);
    let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
    let name = name.map(wide);
    let status = RegSetValueExW(
        key,
        match &name {
            Some(name) => PCWSTR(name.as_ptr()),
            None => PCWSTR::null(),
        },
        0,
        REG_SZ,
        Some(bytes),
    );
    status.0 == 0
}

/// True when the item is already there.
pub fn installed() -> bool {
    unsafe {
        let name = wide(&key_path(KINDS[0]));
        let mut key = HKEY::default();
        let status = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr()), 0, KEY_READ, &mut key);
        if status.0 == 0 {
            let _ = RegCloseKey(key);
            true
        } else {
            false
        }
    }
}

/// Takes the item back out of every kind, leaving no trace behind in Explorer.
pub fn remove() -> bool {
    let mut all = true;
    for kind in KINDS {
        unsafe {
            let name = wide(&key_path(kind));
            // Deleting a key that is not there is a failure too; that is fine,
            // what matters is that it is gone afterwards.
            let status = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr()));
            all &= status.0 == 0;
        }
    }
    all || !installed()
}

/// The one place the choice to keep the item or not is written down. A marker
/// file rather than a registry value, because it sits next to the log that is
/// already there and needs no extra API to read.
fn off_marker() -> Option<std::path::PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(std::path::PathBuf::from(base).join("Picline").join("shellmenu.off"))
}

/// True unless the item was switched off from the tray. Kept across restarts:
/// switching it off and having it come back next launch would be worse than
/// never offering the switch.
pub fn wanted() -> bool {
    !off_marker().map(|path| path.exists()).unwrap_or(false)
}

/// Writes the choice down.
pub fn remember(on: bool) {
    let Some(path) = off_marker() else { return };
    if on {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(&path, b"off\n");
}
