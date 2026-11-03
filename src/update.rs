//! "Is there a newer Picline?"
//!
//! One GET to the GitHub API for the latest release, one comparison, and a
//! question: do you want the page. All of it on a worker thread, because a
//! blocking call on the window thread would freeze the line for as long as the
//! network takes — and this app is supposed to be the thing that never does
//! that.

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNO,
};

use crate::wide;

const HOST: &str = "api.github.com";
const PATH: &str = "/repos/Xinguangcuican/Picline/releases/latest";
const DOWNLOAD: &str = "https://github.com/Xinguangcuican/Picline/releases/latest";

/// Our own version, decided at build time by Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Checks in the background and reports with a message box. The line keeps
/// working while it runs.
pub fn check(owner: HWND) {
    let owner = owner.0 as isize;
    std::thread::spawn(move || {
        let result = fetch();
        let owner = HWND(owner as *mut c_void);
        unsafe {
            match result {
                Ok(tag) => {
                    let latest = tag.trim_start_matches('v');
                    if newer(latest, VERSION) {
                        let text = format!(
                            "有新版本 {latest}（当前 {VERSION}）。\n\n要打开下载页面吗？"
                        );
                        let wide_text = wide(&text);
                        let answer = MessageBoxW(
                            owner,
                            PCWSTR(wide_text.as_ptr()),
                            w!("Picline 更新"),
                            MB_YESNO | MB_ICONINFORMATION,
                        );
                        if answer == IDYES {
                            let url = wide(DOWNLOAD);
                            ShellExecuteW(
                                None,
                                w!("open"),
                                PCWSTR(url.as_ptr()),
                                None,
                                None,
                                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                            );
                        }
                    } else {
                        let text = format!("已经是最新版本（{VERSION}）。");
                        let wide_text = wide(&text);
                        MessageBoxW(
                            owner,
                            PCWSTR(wide_text.as_ptr()),
                            w!("Picline 更新"),
                            MB_OK | MB_ICONINFORMATION,
                        );
                    }
                }
                Err(reason) => {
                    let text = format!("没能检查更新：{reason}");
                    let wide_text = wide(&text);
                    MessageBoxW(
                        owner,
                        PCWSTR(wide_text.as_ptr()),
                        w!("Picline 更新"),
                        MB_OK | MB_ICONWARNING,
                    );
                }
            }
        }
    });
}

fn fetch() -> Result<String, String> {
    let agent = wide(&format!("Picline/{VERSION}"));
    let host = wide(HOST);
    let path = wide(PATH);
    unsafe {
        let session = WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err("无法初始化网络".into());
        }
        // Do not let a dead network hold the thread for a minute.
        let _ = WinHttpSetTimeouts(session, 5000, 5000, 5000, 5000);

        let connect = WinHttpConnect(session, PCWSTR(host.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err("无法连接 GitHub".into());
        }
        let request = WinHttpOpenRequest(
            connect,
            w!("GET"),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err("无法建立请求".into());
        }
        // GitHub answers 403 without a user agent, hence the agent above.
        let mut outcome = Err("请求未发出".to_string());
        if WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok()
            && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok()
        {
            let mut body: Vec<u8> = Vec::new();
            loop {
                let mut available: u32 = 0;
                if WinHttpQueryDataAvailable(request, &mut available).is_err() || available == 0 {
                    break;
                }
                let mut chunk = vec![0u8; available as usize];
                let mut read: u32 = 0;
                if WinHttpReadData(
                    request,
                    chunk.as_mut_ptr() as *mut c_void,
                    available,
                    &mut read,
                )
                .is_err()
                {
                    break;
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                // A few hundred bytes is the whole answer; anything larger is
                // not this endpoint.
                if body.len() > 64 * 1024 {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&body).to_string();
            outcome = match tag_from_json(&text) {
                Some(tag) => Ok(tag),
                None => Err("GitHub 的回复看不懂（可能是限流）".into()),
            };
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);
        outcome
    }
}

/// `"tag_name":"v0.1.23"` out of the release JSON. A regex would be heavier
/// than the one field this needs.
fn tag_from_json(text: &str) -> Option<String> {
    let start = text.find("\"tag_name\"")?;
    let rest = &text[start..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    let tag = &after[..end];
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Whether `candidate` is a later version than `current`, comparing dots as
/// numbers so 0.1.10 comes after 0.1.9 and not before it.
fn newer(candidate: &str, current: &str) -> bool {
    let parse = |text: &str| -> Vec<u32> {
        text.split('.')
            .map(|part| {
                part.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let mut a = parse(candidate);
    let mut b = parse(current);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers_not_strings() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("0.2.0", "0.1.12"));
        assert!(!newer("0.1.9", "0.1.10"));
        assert!(!newer("1.0", "1.0.0"));
    }

    #[test]
    fn the_tag_comes_out_of_the_json() {
        let json = r#"{"url":"x","tag_name":"v0.1.23","name":"Picline 0.1.23"}"#;
        assert_eq!(tag_from_json(json).as_deref(), Some("v0.1.23"));
    }
}
