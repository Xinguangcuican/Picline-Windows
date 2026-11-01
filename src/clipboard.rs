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
}

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
}

impl Shot {
    fn new(width: u32, height: u32, pixels: Vec<u8>) -> Shot {
        let hash = fingerprint(&pixels);
        Shot {
            width,
            height,
            pixels,
            hash,
        }
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
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
    Some(Shot::new(raw.width, raw.height, raw.pixels))
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
