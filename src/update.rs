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
                Ok(json) => {
                    // The tag is the macOS version (v0.2.9); the Windows build
                    // has its own line, so the answer has to come from the
                    // asset names — "Picline-0.1.1-win-x64.exe" and friends.
                    let Some(latest) = windows_version_from_json(&json) else {
                        let text = format!(
                            "最新一次发布里没有 Windows 包。\n\n当前版本 {VERSION}。"
                        );
                        let wide_text = wide(&text);
                        MessageBoxW(
                            owner,
                            PCWSTR(wide_text.as_ptr()),
                            w!("Picline 更新"),
                            MB_OK | MB_ICONINFORMATION,
                        );
                        return;
                    };
                    if newer(&latest, VERSION) {
                        let text = format!(
                            "有新的 Windows 版 {latest}（当前 {VERSION}）。\n\n要打开下载页面吗？"
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
            // "tag_name" is the one field that has to be there; without it the
            // answer is an error page or a rate-limit notice.
            outcome = match tag_from_json(&text) {
                Some(_) => Ok(text),
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

/// The newest Windows version named in a release's asset list.
///
/// A release carries `Picline-<version>-win-x64.exe`, `-win-arm64.exe` and
/// `-win.zip`; the tag beside them belongs to the macOS line. Reading the
/// assets is what keeps "is there a newer Windows build" honest when the two
/// products number themselves apart — which they now do.
fn windows_version_from_json(text: &str) -> Option<String> {
    let mut newest: Option<Vec<u32>> = None;
    let mut newest_text: Option<String> = None;
    let mut rest = text;
    while let Some(at) = rest.find("\"name\"") {
        rest = &rest[at + 6..];
        let Some(_colon) = rest.find(':') else { break };
        let after = rest[rest.find(':')? + 1..].trim_start();
        let Some(after) = after.strip_prefix('"') else { continue };
        let Some(end) = after.find('"') else { break };
        let name = &after[..end];
        if let Some(version) = name
            .strip_prefix("Picline-")
            .and_then(|tail| tail.split_once("-win"))
            .map(|(version, _)| version)
        {
            let parts = numbers(version);
            if newest.as_ref().map(|best| parts > *best).unwrap_or(true) {
                newest = Some(parts);
                newest_text = Some(version.to_string());
            }
        }
    }
    newest_text
}

/// Dotted numbers as a comparable list: "0.1.10" → [0, 1, 10].
fn numbers(text: &str) -> Vec<u32> {
    text.split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// Whether `candidate` is a later version than `current`, comparing dots as
/// numbers so 0.1.10 comes after 0.1.9 and not before it.
fn newer(candidate: &str, current: &str) -> bool {
    let mut a = numbers(candidate);
    let mut b = numbers(current);
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
