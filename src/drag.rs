//! Taking a picture off the line with the mouse.
//!
//! Windows moves files between programs through OLE drag and drop. The source
//! side is two small COM objects: a data object that hands the file over, and
//! a drop source that says when the drag is over. `windows` 0.58 does not
//! generate `_Impl` traits for these two interfaces, so they are written out by
//! hand here — a vtable, a reference count, and a list of wide characters.
//!
//! Only `CF_HDROP` is offered, which is what Explorer, the desktop, and the
//! chat windows all understand: whatever is dropped gets a real file, so
//! dragging to the desktop or a folder copies the picture there, and dragging
//! into WeChat or QQ sends it.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

type Hresult = i32;

const S_OK: Hresult = 0;
const S_FALSE: Hresult = 1;
const E_NOINTERFACE: Hresult = 0x8000_4002u32 as i32;
const E_POINTER: Hresult = 0x8000_4003u32 as i32;
const E_NOTIMPL: Hresult = 0x8000_4001u32 as i32;
const E_OUTOFMEMORY: Hresult = 0x8007_000Eu32 as i32;
const OLE_E_ADVISENOTSUPPORTED: Hresult = 0x8004_0003u32 as i32;
const DATA_S_SAMEFORMATETC: Hresult = 0x0004_0130;
const DV_E_LINDEX: Hresult = 0x8004_0008u32 as i32;
const DV_E_FORMATETC: Hresult = 0x8004_0004u32 as i32;
const DV_E_TYMED: Hresult = 0x8004_0009u32 as i32;
const DV_E_DVASPECT: Hresult = 0x8004_000Bu32 as i32;
const DRAGDROP_S_DROP: Hresult = 0x0004_0100;
const DRAGDROP_S_CANCEL: Hresult = 0x0004_0101;
const DRAGDROP_S_USEDEFAULTCURSORS: Hresult = 0x0004_0102;

const CF_HDROP: u16 = 15;
const DVASPECT_CONTENT: u32 = 1;
const TYMED_HGLOBAL: u32 = 1;
const DATADIR_GET: u32 = 1;
const DROPEFFECT_COPY: u32 = 1;
const MK_LBUTTON: u32 = 0x0001;
const GMEM_MOVEABLE: u32 = 0x0002;
/// `DROPFILES` is five fields; the path list starts right after it.
const DROPFILES_SIZE: usize = 20;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

const IID_UNKNOWN: Guid = Guid {
    d1: 0,
    d2: 0,
    d3: 0,
    d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_DATA_OBJECT: Guid = Guid {
    d1: 0x0000_010E,
    d2: 0,
    d3: 0,
    d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_DROP_SOURCE: Guid = Guid {
    d1: 0x0000_0121,
    d2: 0,
    d3: 0,
    d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_ENUM_FORMATETC: Guid = Guid {
    d1: 0x0000_0103,
    d2: 0,
    d3: 0,
    d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

#[repr(C)]
#[derive(Clone, Copy)]
struct FormatEtc {
    cf_format: u16,
    ptd: *mut c_void,
    dw_aspect: u32,
    l_index: i32,
    tymed: u32,
}

const FORMAT_HDROP: FormatEtc = FormatEtc {
    cf_format: CF_HDROP,
    ptd: std::ptr::null_mut(),
    dw_aspect: DVASPECT_CONTENT,
    l_index: -1,
    tymed: TYMED_HGLOBAL,
};

#[repr(C)]
struct StgMedium {
    tymed: u32,
    handle: *mut c_void,
    unknown: *mut c_void,
}

// The same declarations as in the clipboard module, and deliberately the same
// types: two `extern` blocks for one symbol have to agree.
// The three structures handed across the OLE boundary are laid out here by
// hand, so their sizes are pinned: a wrong size would be a silent corruption
// rather than a compile error.
const _: () = {
    assert!(std::mem::size_of::<Guid>() == 16);
    assert!(std::mem::size_of::<FormatEtc>() == 32);
    assert!(std::mem::size_of::<StgMedium>() == 24);
};

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(handle: isize) -> *mut c_void;
    fn GlobalUnlock(handle: isize) -> i32;
    fn GlobalFree(handle: isize) -> isize;
}

#[link(name = "ole32")]
extern "system" {
    fn OleInitialize(reserved: *mut c_void) -> Hresult;
    fn DoDragDrop(
        data: *mut c_void,
        source: *mut c_void,
        allowed: u32,
        effect: *mut u32,
    ) -> Hresult;
}

/// Called once on the thread that will start drags. Without it `DoDragDrop`
/// fails with `CO_E_NOTINITIALIZED` and nothing happens at all.
pub fn prepare() {
    let hr = unsafe { OleInitialize(std::ptr::null_mut()) };
    // S_FALSE means someone got here first, which is just as good.
    crate::clipboard::trace(&format!("OLE initialized: {hr:#010x}"));
}

// --- the data object -------------------------------------------------------

#[repr(C)]
struct DataVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium) -> Hresult,
    get_data_here: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut StgMedium) -> Hresult,
    query_get_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc) -> Hresult,
    get_canonical_format_etc:
        unsafe extern "system" fn(*mut c_void, *const FormatEtc, *mut FormatEtc) -> Hresult,
    set_data: unsafe extern "system" fn(*mut c_void, *const FormatEtc, *const StgMedium, i32) -> Hresult,
    enum_format_etc: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
    d_advise: unsafe extern "system" fn(
        *mut c_void,
        *const FormatEtc,
        u32,
        *mut c_void,
        *mut u32,
    ) -> Hresult,
    d_unadvise: unsafe extern "system" fn(*mut c_void, u32) -> Hresult,
    enum_d_advise: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hresult,
}

#[repr(C)]
struct Data {
    vtbl: *const DataVtbl,
    refs: i32,
    /// Absolute path, wide, ending in two nulls the way `DROPFILES` wants it.
    path: Vec<u16>,
}

static DATA_VTBL: DataVtbl = DataVtbl {
    query_interface: data_query_interface,
    add_ref: data_add_ref,
    release: data_release,
    get_data: data_get_data,
    get_data_here: data_get_data_here,
    query_get_data: data_query_get_data,
    get_canonical_format_etc: data_get_canonical_format_etc,
    set_data: data_set_data,
    enum_format_etc: data_enum_format_etc,
    d_advise: data_d_advise,
    d_unadvise: data_d_unadvise,
    enum_d_advise: data_enum_d_advise,
};

/// Everything this object can hand out, and why a request for something else
/// is refused. Returning the specific complaint helps when a target refuses
/// the drop for reasons of its own.
fn check(asked: &FormatEtc) -> Option<Hresult> {
    if asked.l_index != -1 {
        return Some(DV_E_LINDEX);
    }
    if asked.dw_aspect != DVASPECT_CONTENT {
        return Some(DV_E_DVASPECT);
    }
    if asked.tymed & TYMED_HGLOBAL == 0 {
        return Some(DV_E_TYMED);
    }
    if asked.cf_format != CF_HDROP {
        return Some(DV_E_FORMATETC);
    }
    None
}

unsafe extern "system" fn data_query_interface(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> Hresult {
    if iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_UNKNOWN || *iid == IID_DATA_OBJECT {
        data_add_ref(this);
        *out = this;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn data_add_ref(this: *mut c_void) -> u32 {
    let data = &mut *(this as *mut Data);
    data.refs += 1;
    data.refs as u32
}

unsafe extern "system" fn data_release(this: *mut c_void) -> u32 {
    let data = this as *mut Data;
    (*data).refs -= 1;
    if (*data).refs <= 0 {
        drop(Box::from_raw(data));
        return 0;
    }
    (*data).refs as u32
}

/// The `CF_HDROP` payload: a `DROPFILES` header followed by the path list,
/// wide and ending in two nulls. Read from offset 20 on, which is what every
/// drop target does.
fn drop_blob(path: &[u16]) -> Vec<u8> {
    let mut blob = vec![0u8; DROPFILES_SIZE + path.len() * 2];
    // pFiles: where the list starts. pt and fNC stay zero.
    blob[0..4].copy_from_slice(&(DROPFILES_SIZE as u32).to_le_bytes());
    // fWide: the list is UTF-16, not the ANSI of 1995.
    blob[16..20].copy_from_slice(&1i32.to_le_bytes());
    for (index, unit) in path.iter().enumerate() {
        let at = DROPFILES_SIZE + index * 2;
        blob[at..at + 2].copy_from_slice(&unit.to_le_bytes());
    }
    blob
}

unsafe extern "system" fn data_get_data(
    this: *mut c_void,
    asked: *const FormatEtc,
    out: *mut StgMedium,
) -> Hresult {
    if asked.is_null() || out.is_null() {
        return E_POINTER;
    }
    if let Some(why) = check(&*asked) {
        return why;
    }
    let data = &*(this as *const Data);
    let blob = drop_blob(&data.path);

    let handle = GlobalAlloc(GMEM_MOVEABLE, blob.len());
    if handle == 0 {
        return E_OUTOFMEMORY;
    }
    let base = GlobalLock(handle) as *mut u8;
    if base.is_null() {
        GlobalFree(handle);
        return E_OUTOFMEMORY;
    }
    std::ptr::copy_nonoverlapping(blob.as_ptr(), base, blob.len());
    GlobalUnlock(handle);

    *out = StgMedium {
        tymed: TYMED_HGLOBAL,
        handle: handle as *mut c_void,
        unknown: std::ptr::null_mut(),
    };
    S_OK
}

unsafe extern "system" fn data_get_data_here(
    _this: *mut c_void,
    _asked: *const FormatEtc,
    _out: *mut StgMedium,
) -> Hresult {
    E_NOTIMPL
}

unsafe extern "system" fn data_query_get_data(
    _this: *mut c_void,
    asked: *const FormatEtc,
) -> Hresult {
    if asked.is_null() {
        return E_POINTER;
    }
    check(&*asked).unwrap_or(S_OK)
}

unsafe extern "system" fn data_get_canonical_format_etc(
    _this: *mut c_void,
    _asked: *const FormatEtc,
    out: *mut FormatEtc,
) -> Hresult {
    // No conversion is ever needed, which `DATA_S_SAMEFORMATETC` reports.
    if !out.is_null() {
        *out = FORMAT_HDROP;
    }
    DATA_S_SAMEFORMATETC
}

unsafe extern "system" fn data_set_data(
    _this: *mut c_void,
    _asked: *const FormatEtc,
    _medium: *const StgMedium,
    _release: i32,
) -> Hresult {
    E_NOTIMPL
}

unsafe extern "system" fn data_d_advise(
    _this: *mut c_void,
    _asked: *const FormatEtc,
    _flags: u32,
    _sink: *mut c_void,
    connection: *mut u32,
) -> Hresult {
    // No change notifications: the list of paths never changes while it is
    // being carried.
    if !connection.is_null() {
        *connection = 0;
    }
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn data_d_unadvise(_this: *mut c_void, _connection: u32) -> Hresult {
    E_NOTIMPL
}

unsafe extern "system" fn data_enum_d_advise(
    _this: *mut c_void,
    out: *mut *mut c_void,
) -> Hresult {
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    OLE_E_ADVISENOTSUPPORTED
}

// --- the one item enumerator ----------------------------------------------
//
// Only `CF_HDROP` is on offer, but some drop targets enumerate the formats
// before they ask for one, and a target that gets `E_NOTIMPL` from here
// sometimes gives up on the drop entirely.

#[repr(C)]
struct EnumVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    next: unsafe extern "system" fn(*mut c_void, u32, *mut FormatEtc, *mut u32) -> Hresult,
    skip: unsafe extern "system" fn(*mut c_void, u32) -> Hresult,
    reset: unsafe extern "system" fn(*mut c_void) -> Hresult,
    clone: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> Hresult,
}

#[repr(C)]
struct Enum {
    vtbl: *const EnumVtbl,
    refs: i32,
    /// 0 = the item is still ahead, 1 = it has been handed out.
    given: u32,
}

static ENUM_VTBL: EnumVtbl = EnumVtbl {
    query_interface: enum_query_interface,
    add_ref: enum_add_ref,
    release: enum_release,
    next: enum_next,
    skip: enum_skip,
    reset: enum_reset,
    clone: enum_clone,
};

unsafe fn new_enum(given: u32) -> *mut c_void {
    Box::into_raw(Box::new(Enum {
        vtbl: &ENUM_VTBL,
        refs: 1,
        given,
    })) as *mut c_void
}

unsafe extern "system" fn enum_query_interface(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> Hresult {
    if iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_UNKNOWN || *iid == IID_ENUM_FORMATETC {
        enum_add_ref(this);
        *out = this;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn enum_add_ref(this: *mut c_void) -> u32 {
    let item = &mut *(this as *mut Enum);
    item.refs += 1;
    item.refs as u32
}

unsafe extern "system" fn enum_release(this: *mut c_void) -> u32 {
    let item = this as *mut Enum;
    (*item).refs -= 1;
    if (*item).refs <= 0 {
        drop(Box::from_raw(item));
        return 0;
    }
    (*item).refs as u32
}

unsafe extern "system" fn enum_next(
    this: *mut c_void,
    wanted: u32,
    out: *mut FormatEtc,
    fetched: *mut u32,
) -> Hresult {
    let item = &mut *(this as *mut Enum);
    let mut given = 0;
    while given < wanted && item.given < 1 {
        if !out.is_null() {
            *out.add(given as usize) = FORMAT_HDROP;
        }
        item.given += 1;
        given += 1;
    }
    if !fetched.is_null() {
        *fetched = given;
    }
    if given == wanted {
        S_OK
    } else {
        S_FALSE
    }
}

unsafe extern "system" fn enum_skip(this: *mut c_void, count: u32) -> Hresult {
    let item = &mut *(this as *mut Enum);
    item.given = (item.given + count).min(1);
    if item.given >= 1 && count > 0 {
        // Nothing left to skip over.
        S_FALSE
    } else {
        S_OK
    }
}

unsafe extern "system" fn enum_reset(this: *mut c_void) -> Hresult {
    (*(this as *mut Enum)).given = 0;
    S_OK
}

unsafe extern "system" fn enum_clone(this: *mut c_void, out: *mut *mut c_void) -> Hresult {
    if out.is_null() {
        return E_POINTER;
    }
    let item = &*(this as *const Enum);
    *out = new_enum(item.given);
    S_OK
}

unsafe extern "system" fn data_enum_format_etc(
    _this: *mut c_void,
    direction: u32,
    out: *mut *mut c_void,
) -> Hresult {
    if direction != DATADIR_GET {
        return E_NOTIMPL;
    }
    if out.is_null() {
        return E_POINTER;
    }
    *out = new_enum(0);
    S_OK
}

// --- the drop source -------------------------------------------------------

#[repr(C)]
struct SourceVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    query_continue_drag: unsafe extern "system" fn(*mut c_void, i32, u32) -> Hresult,
    give_feedback: unsafe extern "system" fn(*mut c_void, u32) -> Hresult,
}

#[repr(C)]
struct Source {
    vtbl: *const SourceVtbl,
    refs: i32,
}

static SOURCE_VTBL: SourceVtbl = SourceVtbl {
    query_interface: source_query_interface,
    add_ref: source_add_ref,
    release: source_release,
    query_continue_drag: source_query_continue_drag,
    give_feedback: source_give_feedback,
};

unsafe extern "system" fn source_query_interface(
    this: *mut c_void,
    iid: *const Guid,
    out: *mut *mut c_void,
) -> Hresult {
    if iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    if *iid == IID_UNKNOWN || *iid == IID_DROP_SOURCE {
        source_add_ref(this);
        *out = this;
        return S_OK;
    }
    *out = std::ptr::null_mut();
    E_NOINTERFACE
}

unsafe extern "system" fn source_add_ref(this: *mut c_void) -> u32 {
    let source = &mut *(this as *mut Source);
    source.refs += 1;
    source.refs as u32
}

unsafe extern "system" fn source_release(this: *mut c_void) -> u32 {
    let source = this as *mut Source;
    (*source).refs -= 1;
    if (*source).refs <= 0 {
        drop(Box::from_raw(source));
        return 0;
    }
    (*source).refs as u32
}

unsafe extern "system" fn source_query_continue_drag(
    _this: *mut c_void,
    escape: i32,
    keys: u32,
) -> Hresult {
    if escape != 0 {
        return DRAGDROP_S_CANCEL;
    }
    if keys & MK_LBUTTON == 0 {
        return DRAGDROP_S_DROP;
    }
    S_OK
}

unsafe extern "system" fn source_give_feedback(_this: *mut c_void, _effect: u32) -> Hresult {
    DRAGDROP_S_USEDEFAULTCURSORS
}

/// Runs the drag and blocks until the button comes up. The result is written
/// as `hr / effect` for the log, so a drop that lands nowhere can be told from
/// one that never started.
pub fn carry(path: &Path) -> String {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if !wide.contains(&0) {
        wide.push(0);
        wide.push(0);
    }

    let data = Box::into_raw(Box::new(Data {
        vtbl: &DATA_VTBL,
        refs: 1,
        path: wide,
    }));
    let source = Box::into_raw(Box::new(Source {
        vtbl: &SOURCE_VTBL,
        refs: 1,
    }));

    let mut effect: u32 = DROPEFFECT_COPY;
    let hr = unsafe {
        DoDragDrop(
            data as *mut c_void,
            source as *mut c_void,
            DROPEFFECT_COPY,
            &mut effect,
        )
    };

    unsafe {
        data_release(data as *mut c_void);
        source_release(source as *mut c_void);
    }

    format!("hr {hr:#010x}, effect {effect}")
}
