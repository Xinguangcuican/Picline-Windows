//! Pictures that GDI+ cannot read, read through Windows Imaging Component.
//!
//! GDI+ knows PNG, JPEG, GIF, BMP, TIFF and ICO and nothing else, so a WebP from
//! a browser or a HEIC from an iPhone simply failed to load. WIC asks whatever
//! codecs the machine has installed, which covers WebP on Windows 10 and later,
//! and HEIC as soon as the HEIF extension from the Store is there — no decoder
//! of our own, no extra megabytes in the exe.
//!
//! Everything here is best effort: a missing codec returns None and the caller
//! falls back, or gives up, as it did before.

use std::path::Path;

use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

use crate::gdiplus::Raw;

/// Longest side worth keeping in memory, the same ceiling GDI+ loading uses.
const MAX_SIDE: u32 = 1400;

/// Decodes `path` through whichever WIC codec claims it. `None` when no codec
/// on this machine can.
pub fn decode(path: &Path) -> Option<Raw> {
    use std::os::windows::ffi::OsStrExt;
    let mut file: Vec<u16> = path.as_os_str().encode_wide().collect();
    file.push(0);
    unsafe { decode_wide(&file) }
}

unsafe fn decode_wide(file: &[u16]) -> Option<Raw> {
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let decoder = factory
        .CreateDecoderFromFilename(
            windows::core::PCWSTR(file.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
        .ok()?;
    let frame = decoder.GetFrame(0).ok()?;

    let mut width = 0u32;
    let mut height = 0u32;
    frame.GetSize(&mut width, &mut height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }

    // One format for everything: 32-bit BGRA, rows top down, which is what the
    // rest of the app already draws with.
    let converter = factory.CreateFormatConverter().ok()?;
    converter
        .Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .ok()?;

    let stride = width * 4;
    let mut pixels = vec![0u8; (stride * height) as usize];
    let rect = windows::Win32::Graphics::Imaging::WICRect {
        X: 0,
        Y: 0,
        Width: width as i32,
        Height: height as i32,
    };
    converter
        .CopyPixels(&rect, stride, &mut pixels)
        .ok()?;

    let raw = Raw {
        width,
        height,
        pixels,
    };
    if width.max(height) <= MAX_SIDE {
        Some(raw)
    } else {
        // Same ceiling as the GDI+ path, so a 8000 px HEIC does not sit in
        // memory at full size while the line only ever draws it 140 px wide.
        crate::gdiplus::shrink(raw.width, raw.height, &raw.pixels).or(Some(raw))
    }
}
