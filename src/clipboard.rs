//! Everything between the Windows clipboard and a row of pixels.
//!
//! Apps do not agree on how to put a picture there. A screenshot tool writes a
//! DIB, Explorer writes a list of files, some tools write PNG, a few write an
//! HBITMAP. All of them are read here and turned into the same thing: a flat
//! buffer of BGRA, top row first.

use std::ffi::c_void;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};

const CF_BITMAP: u32 = 2;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(owner: isize) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, mem: isize) -> isize;
    fn GetClipboardData(format: u32) -> isize;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn EnumClipboardFormats(format: u32) -> u32;
    fn GetClipboardFormatNameW(format: u32, buffer: *mut u16, max: i32) -> i32;
    fn AddClipboardFormatListener(owner: isize) -> i32;
}

/// Asks for a `WM_CLIPBOARDUPDATE` every time the clipboard changes.
pub fn watch(owner: isize) {
    unsafe {
        let _ = AddClipboardFormatListener(owner);
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalLock(handle: isize) -> *mut c_void;
    fn GlobalUnlock(handle: isize) -> i32;
    fn GlobalSize(handle: isize) -> usize;
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalFree(handle: isize) -> isize;
}

const GMEM_MOVEABLE: u32 = 0x0002;
const GMEM_ZEROINIT: u32 = 0x0040;

#[link(name = "shell32")]
extern "system" {
    fn DragQueryFileW(drop: isize, index: u32, buffer: *mut u16, length: u32) -> u32;
}

#[link(name = "gdi32")]
extern "system" {
    fn GetDIBits(
        dc: isize,
        bitmap: isize,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut Bmi,
        usage: u32,
    ) -> i32;
    fn GetObjectW(object: isize, size: i32, out: *mut c_void) -> i32;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BmiHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    used: u32,
    important: u32,
}

/// `BITMAPINFO`: the header plus room for a palette we never use. Kept on one
/// struct because the API takes them as a single block.
#[repr(C)]
#[derive(Clone, Copy)]
struct Bmi {
    header: BmiHeader,
    colors: [u32; 260],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct GdiBitmap {
    kind: u32,
    width: i32,
    height: i32,
    width_bytes: i32,
    planes: u16,
    bits: u16,
    bits_pointer: *mut c_void,
}

/// One picture off the clipboard.
pub struct Shot {
    pub width: u32,
    pub height: u32,
    /// BGRA, top row first, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
    pub hash: u64,
    /// Where it came from, when it came from a file rather than the clipboard.
    /// The caption in the preview and "show in Explorer" both need it.
    pub path: Option<PathBuf>,
}

impl Shot {
    fn new(width: u32, height: u32, pixels: Vec<u8>) -> Shot {
        let hash = fingerprint(&pixels);
        Shot {
            width,
            height,
            pixels,
            hash,
            path: None,
        }
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    /// The line under a preview: what it is and how big it is.
    pub fn caption(&self) -> String {
        let name = match &self.path {
            Some(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "图片".to_string()),
            None => "截图 / 复制的图片".to_string(),
        };
        format!("{name} · {} × {}", self.width, self.height)
    }
}

/// Reads the clipboard. `None` means the clipboard was busy and nothing could
/// be read; `Some` with an empty list means it held no picture.
pub fn read(owner: isize) -> Option<Vec<Shot>> {
    unsafe {
        if OpenClipboard(owner) == 0 {
            return None;
        }
        let mut shots: Vec<Shot> = Vec::new();
        let png = register("PNG");
        // Most clipboard changes are text; skipping those keeps the log short
        // and stops it being rewritten every time something is copied.
        let picture_format = IsClipboardFormatAvailable(CF_DIB) != 0
            || IsClipboardFormatAvailable(CF_DIBV5) != 0
            || IsClipboardFormatAvailable(CF_BITMAP) != 0
            || IsClipboardFormatAvailable(CF_HDROP) != 0
            || (png != 0 && IsClipboardFormatAvailable(png) != 0);
        if !picture_format {
            CloseClipboard();
            return Some(Vec::new());
        }
        trace(&format!("clipboard holds: {}", list_formats()));

        // A file copy in Explorer gives a list of paths and nothing else.
        if IsClipboardFormatAvailable(CF_HDROP) != 0 {
            if let Some(drop) = handle(CF_HDROP) {
                from_files(drop, &mut shots);
            }
        }
        if shots.is_empty() {
            for format in [CF_DIBV5, CF_DIB] {
                if IsClipboardFormatAvailable(format) == 0 {
                    continue;
                }
                if let Some(mem) = handle(format) {
                    if let Some(shot) = from_dib(mem) {
                        shots.push(shot);
                        break;
                    }
                }
            }
        }
        if shots.is_empty() && IsClipboardFormatAvailable(CF_BITMAP) != 0 {
            if let Some(bitmap) = handle(CF_BITMAP) {
                if let Some(shot) = from_bitmap(bitmap) {
                    shots.push(shot);
                }
            }
        }
        if shots.is_empty() {
            if png != 0 && IsClipboardFormatAvailable(png) != 0 {
                if let Some(mem) = handle(png) {
                    if let Some(shot) = from_png(mem) {
                        shots.push(shot);
                    }
                }
            }
        }

        CloseClipboard();

        let sizes: Vec<String> = shots
            .iter()
            .map(|s| format!("{}x{}", s.width, s.height))
            .collect();
        trace(&format!("took {} picture(s) {}", shots.len(), sizes.join(" ")));
        Some(shots)
    }
}

unsafe fn handle(format: u32) -> Option<isize> {
    let value = GetClipboardData(format);
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

unsafe fn list_formats() -> String {
    let mut names: Vec<String> = Vec::new();
    let mut format = EnumClipboardFormats(0);
    while format != 0 {
        let mut buffer = [0u16; 96];
        let length = GetClipboardFormatNameW(format, buffer.as_mut_ptr(), buffer.len() as i32);
        if length > 0 {
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        } else {
            names.push(
                match format {
                    CF_BITMAP => "CF_BITMAP".to_string(),
                    CF_DIB => "CF_DIB".to_string(),
                    CF_HDROP => "CF_HDROP".to_string(),
                    CF_DIBV5 => "CF_DIBV5".to_string(),
                    other => format!("format {other}"),
                },
            );
        }
        format = EnumClipboardFormats(format);
    }
    names.join(", ")
}

unsafe fn register(name: &str) -> u32 {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    RegisterClipboardFormatW(wide.as_ptr())
}

// --- the formats ----------------------------------------------------------

/// Explorer's list of copied files. Only pictures are taken.
unsafe fn from_files(drop: isize, out: &mut Vec<Shot>) {
    let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
    for index in 0..count {
        let length = DragQueryFileW(drop, index, std::ptr::null_mut(), 0);
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let written = DragQueryFileW(drop, index, buffer.as_mut_ptr(), length + 1);
        if written == 0 {
            continue;
        }
        buffer.truncate(written as usize);
        let path: PathBuf = std::ffi::OsString::from_wide(&buffer).into();
        if let Some(shot) = shot_from_path(&path) {
            out.push(shot);
        }
    }
}

/// One picture from a file on disk. This is the same path a copied file and
/// Explorer's "挂到 Picline" both take, so a picture looks the same whichever
/// way it arrived.
pub fn shot_from_path(path: &std::path::Path) -> Option<Shot> {
    if !is_picture(path) {
        return None;
    }
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let raw = crate::gdiplus::image_from_file(&wide)?;
    let mut shot = Shot::new(raw.width, raw.height, raw.pixels);
    shot.path = Some(path.to_path_buf());
    Some(shot)
}

fn is_picture(path: &std::path::Path) -> bool {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "bmp" | "gif" | "tif" | "tiff") => true,
        _ => false,
    }
}

/// A `BITMAPINFOHEADER` followed by its bits: what screenshot tools write.
unsafe fn from_dib(mem: isize) -> Option<Shot> {
    let size = GlobalSize(mem);
    if size < 40 {
        return None;
    }
    let base = GlobalLock(mem) as *const u8;
    if base.is_null() {
        return None;
    }
    let shot = unpack_dib(base, size);
    GlobalUnlock(mem);
    shot
}

unsafe fn unpack_dib(base: *const u8, size: usize) -> Option<Shot> {
    let header = std::slice::from_raw_parts(base, 40);
    let read_u32 = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    let read_u16 = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);

    let header_size = read_u32(0) as usize;
    let width = read_u32(4) as i32;
    let raw_height = read_u32(8) as i32;
    let bit_count = read_u16(14) as u32;
    let compression = read_u32(16);

    if header_size < 40 || header_size > size || width <= 0 || raw_height == 0 {
        return None;
    }
    let top_down = raw_height < 0;
    let height = raw_height.unsigned_abs();
    let bytes = match bit_count {
        32 => 4usize,
        24 => 3,
        _ => return None,
    };
    let stride = ((width as usize * bit_count as usize + 31) / 32) * 4;
    let mut offset = header_size;
    // Three channel masks sit between the header and the bits.
    if compression == 3 && header_size < 124 {
        offset += 12;
    }
    let needed = offset.checked_add(stride.checked_mul(height as usize)?)?;
    if needed > size {
        return None;
    }

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let row_bytes = width as usize * 4;
    let mut any_alpha = false;
    for row in 0..height as usize {
        let source_row = if top_down { row } else { height as usize - 1 - row };
        let source = base.add(offset + source_row * stride);
        let target = pixels.as_mut_ptr().add(row * row_bytes);
        if bytes == 4 {
            std::ptr::copy_nonoverlapping(source, target, row_bytes);
            let mut at = 3;
            while at < row_bytes {
                if *target.add(at) != 0 {
                    any_alpha = true;
                    break;
                }
                at += 4;
            }
        } else {
            for column in 0..width as usize {
                *target.add(column * 4) = *source.add(column * 3);
                *target.add(column * 4 + 1) = *source.add(column * 3 + 1);
                *target.add(column * 4 + 2) = *source.add(column * 3 + 2);
                *target.add(column * 4 + 3) = 255;
            }
        }
    }
    // Plenty of tools leave the alpha channel at zero on a 32-bit image; if
    // nothing in it is set, the picture is meant to be opaque.
    if bytes == 4 && !any_alpha {
        let mut at = 3;
        while at < pixels.len() {
            pixels[at] = 255;
            at += 4;
        }
    }

    finish(width as u32, height, pixels)
}

/// A raw `HBITMAP`, which GDI hands over without any header of its own.
unsafe fn from_bitmap(bitmap: isize) -> Option<Shot> {
    let mut info = GdiBitmap::default();
    if GetObjectW(bitmap, std::mem::size_of::<GdiBitmap>() as i32, &mut info as *mut _ as *mut c_void)
        == 0
    {
        return None;
    }
    if info.width <= 0 || info.height <= 0 {
        return None;
    }
    let width = info.width;
    let height = info.height;

    let mut block = Bmi {
        header: BmiHeader {
            size: std::mem::size_of::<BmiHeader>() as u32,
            width,
            // Negative: ask GDI for rows top down, which is how we store them.
            height: -height,
            planes: 1,
            bit_count: 32,
            compression: 0,
            ..Default::default()
        },
        colors: [0; 260],
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let dc = GetDC(None);
    let lines = GetDIBits(
        dc.0 as isize,
        bitmap,
        0,
        height as u32,
        pixels.as_mut_ptr() as *mut c_void,
        &mut block,
        0,
    );
    ReleaseDC(None, dc);
    if lines == 0 {
        return None;
    }
    finish(width as u32, height as u32, pixels)
}

/// PNG written straight to the clipboard, which skips the DIB dance entirely.
unsafe fn from_png(mem: isize) -> Option<Shot> {
    let size = GlobalSize(mem);
    if size < 8 {
        return None;
    }
    let base = GlobalLock(mem) as *const u8;
    if base.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(base, size).to_vec();
    GlobalUnlock(mem);
    let raw = crate::gdiplus::image_from_stream(&bytes)?;
    Some(Shot::new(raw.width, raw.height, raw.pixels))
}

/// Shared last step: keep it a sane size, then wrap it up.
fn finish(width: u32, height: u32, pixels: Vec<u8>) -> Option<Shot> {
    match crate::gdiplus::shrink(width, height, &pixels) {
        Some(raw) => Some(Shot::new(raw.width, raw.height, raw.pixels)),
        None => Some(Shot::new(width, height, pixels)),
    }
}

/// A cheap identity for a picture: enough to spot the same one twice, and far
/// cheaper than hashing every byte of a screen sized buffer.
fn fingerprint(pixels: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let step = (pixels.len() / 512).max(4);
    let mut at = 0;
    while at < pixels.len() {
        hash ^= pixels[at] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        at += step;
    }
    hash ^= pixels.len() as u64;
    hash
}

// --- giving pictures back -------------------------------------------------

/// Puts the picture on the clipboard as a `CF_DIB`, which is what WeChat, QQ
/// and Paint read when you press Ctrl+V. Rows are bottom up here, as the
/// format expects, so the memory can be handed over untouched.
pub fn put_image(shot: &Shot) -> bool {
    let row = shot.width as usize * 4;
    let total = 40 + row * shot.height as usize;
    unsafe {
        if OpenClipboard(0) == 0 {
            return false;
        }
        let _ = EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total);
        let mut done = false;
        if mem != 0 {
            let base = GlobalLock(mem) as *mut u8;
            if base.is_null() {
                let _ = GlobalFree(mem);
            } else {
                write_u32(base, 0, 40); // biSize
                write_u32(base, 4, shot.width); // biWidth
                write_u32(base, 8, shot.height); // biHeight, bottom up
                write_u16(base, 12, 1); // biPlanes
                write_u16(base, 14, 32); // biBitCount
                write_u32(base, 16, 0); // BI_RGB
                write_u32(base, 20, (row * shot.height as usize) as u32);
                for y in 0..shot.height as usize {
                    let from = shot.height as usize - 1 - y;
                    let source = shot.pixels.as_ptr().add(from * row);
                    std::ptr::copy_nonoverlapping(source, base.add(40 + y * row), row);
                }
                let _ = GlobalUnlock(mem);
                // On success the clipboard owns the block; freeing it would
                // leave the data dangling.
                done = SetClipboardData(CF_DIB, mem) != 0;
                if !done {
                    let _ = GlobalFree(mem);
                }
            }
        }
        CloseClipboard();
        trace(&format!("copied picture to clipboard: {done}"));
        done
    }
}

/// Puts files on the clipboard, so a picture can be pasted into a chat as a
/// file, or straight into Explorer.
pub fn put_files(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    // DROPFILES: header, then a double terminated list of UTF-16 paths.
    let mut names: Vec<u16> = Vec::new();
    for path in paths {
        names.extend(path.as_os_str().encode_wide());
        names.push(0);
    }
    names.push(0);
    let header = 20usize;
    let total = header + names.len() * 2;
    unsafe {
        if OpenClipboard(0) == 0 {
            return false;
        }
        let _ = EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total);
        let mut done = false;
        if mem != 0 {
            let base = GlobalLock(mem) as *mut u8;
            if base.is_null() {
                let _ = GlobalFree(mem);
            } else {
                write_u32(base, 0, header as u32); // pFiles
                write_u32(base, 16, 1); // fWide
                std::ptr::copy_nonoverlapping(
                    names.as_ptr() as *const u8,
                    base.add(header),
                    names.len() * 2,
                );
                let _ = GlobalUnlock(mem);
                done = SetClipboardData(CF_HDROP, mem) != 0;
                if !done {
                    let _ = GlobalFree(mem);
                }
            }
        }
        CloseClipboard();
        trace(&format!("copied {} file(s) to clipboard: {done}", paths.len()));
        done
    }
}

unsafe fn write_u32(base: *mut u8, at: usize, value: u32) {
    std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), base.add(at), 4);
}

unsafe fn write_u16(base: *mut u8, at: usize, value: u16) {
    std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), base.add(at), 2);
}

/// A file on disk for a picture that only exists in memory. Screenshots have
/// no file of their own; anything that needs one — pasting as a file, saving,
/// showing in Explorer — gets this instead.
pub fn ensure_file(shot: &Shot) -> Option<PathBuf> {
    if let Some(path) = &shot.path {
        if path.is_file() {
            return Some(path.clone());
        }
    }
    let folder = std::env::temp_dir().join("Picline");
    std::fs::create_dir_all(&folder).ok()?;
    let path = folder.join(format!("picline-{:016x}.png", shot.hash));
    if path.is_file() {
        return Some(path);
    }
    if crate::gdiplus::save_png(&shot.pixels, shot.width, shot.height, &path) {
        Some(path)
    } else {
        None
    }
}

/// Saves a copy on the Desktop, named the way a screenshot tool would.
pub fn save_to_desktop(shot: &Shot) -> Option<PathBuf> {
    let desktop = desktop_dir()?;
    let stamp = timestamp();
    let path = desktop.join(format!("Picline {stamp}.png"));
    if crate::gdiplus::save_png(&shot.pixels, shot.width, shot.height, &path) {
        Some(path)
    } else {
        None
    }
}

fn desktop_dir() -> Option<PathBuf> {
    let base = std::env::var("USERPROFILE").ok()?;
    let path = PathBuf::from(base).join("Desktop");
    if path.is_dir() {
        return Some(path);
    }
    // A Desktop redirected into OneDrive is common enough to be worth a look.
    let alt = PathBuf::from(std::env::var("OneDrive").ok()?).join("Desktop");
    if alt.is_dir() {
        Some(alt)
    } else {
        None
    }
}

/// `2026-11-02 07-23-45`, from the system clock, in local time.
fn timestamp() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLocalTime(out: *mut SystemTime);
    }
    let mut now = SystemTime::default();
    unsafe { GetLocalTime(&mut now) };
    format!(
        "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        now.year, now.month, now.day, now.hour, now.minute, now.second
    )
}

// --- the log --------------------------------------------------------------

/// A small log next to the executable's data, so a machine we cannot reach can
/// still tell us what the clipboard held.
pub fn trace(line: &str) {
    use std::io::Write;
    let Ok(base) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let folder = PathBuf::from(base).join("Picline");
    let _ = std::fs::create_dir_all(&folder);
    let path = folder.join("picline.log");
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 256 * 1024 {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}
