//! Launch at login.
//!
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` is the one place a
//! per-user app can put itself without an installer or administrator rights.
//! One value, removed when the user says stop — nothing else is touched.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("Picline");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, access, &mut key) };
    (status == ERROR_SUCCESS).then_some(key)
}

/// Whether Picline is set to start with Windows right now.
pub fn enabled() -> bool {
    let Some(key) = open(KEY_QUERY_VALUE) else {
        return false;
    };
    let mut probe: u32 = 0;
    let mut size: u32 = 0;
    let status = unsafe {
        RegQueryValueExW(key, VALUE, None, None, Some(&mut probe as *mut u32 as *mut u8), Some(&mut size))
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    // Any failure, including ERROR_FILE_NOT_FOUND for a missing value, means
    // "not enabled".
    status == ERROR_SUCCESS
}

/// Writes or removes the value. Returns whether it worked, so the tray menu can
/// stay honest about the state.
pub fn set(on: bool) -> bool {
    let Some(key) = open(KEY_SET_VALUE) else {
        return false;
    };
    let ok = unsafe {
        if on {
            match std::env::current_exe() {
                Ok(path) => {
                    // Quoted, because the folder name has a space in it for
                    // most people who install to Program Files.
                    let command = format!("\"{}\"", path.display());
                    let mut data: Vec<u16> = command.encode_utf16().collect();
                    data.push(0);
                    let bytes = std::slice::from_raw_parts(
                        data.as_ptr() as *const u8,
                        data.len() * 2,
                    );
                    RegSetValueExW(key, VALUE, 0, REG_SZ, Some(bytes)) == ERROR_SUCCESS
                }
                Err(_) => false,
            }
        } else {
            let status = RegDeleteValueW(key, VALUE);
            status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    ok
}


