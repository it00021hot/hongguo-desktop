use std::{ffi::c_void, sync::OnceLock};

use hongguo_common::{AppError, AppResult};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        Media::MediaFoundation::{
            IMFActivate, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader,
            MFT_REGISTER_TYPE_INFO,
        },
        System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
    },
    core::{GUID, HRESULT, PCSTR, PCWSTR, Type, w},
};

use super::{mft::hr, symbols};

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type CreateObject = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateBuffer = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type EnumTransforms = unsafe extern "system" fn(
    GUID,
    u32,
    *const MFT_REGISTER_TYPE_INFO,
    *const MFT_REGISTER_TYPE_INFO,
    *mut *mut Option<IMFActivate>,
    *mut u32,
) -> HRESULT;
type CreateReader = unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;

struct Library(HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: this handle owns exactly one successful LoadLibraryExW reference.
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

struct Api {
    startup: Startup,
    media_type: CreateObject,
    sample: CreateObject,
    memory_buffer: CreateBuffer,
    enum_transforms: EnumTransforms,
    source_reader: CreateReader,
    _libraries: [Library; 2],
}

// Loaded modules and exported functions are process-wide; their owning handles
// never move or unload while the shared Api is reachable.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

impl Api {
    fn load() -> Result<Self, String> {
        // SAFETY: fixed DLL basenames are resolved only in the OS System32 directory.
        let platform = Library(
            unsafe { LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|error| format!("mfplat.dll: {error}"))?,
        );
        let reader = Library(
            unsafe { LoadLibraryExW(w!("mfreadwrite.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|error| format!("mfreadwrite.dll: {error}"))?,
        );
        let names = [
            c"MFStartup",
            c"MFCreateMediaType",
            c"MFCreateSample",
            c"MFCreateMemoryBuffer",
            c"MFTEnumEx",
            c"MFCreateSourceReaderFromURL",
        ];
        let resolved = symbols::resolve(&names, |name| {
            let module = if name == c"MFCreateSourceReaderFromURL" {
                reader.0
            } else {
                platform.0
            };
            // SAFETY: names are static NUL-terminated SDK exports; modules remain owned.
            unsafe { GetProcAddress(module, PCSTR(name.as_ptr().cast())) }
        })?;
        let [
            startup,
            media_type,
            sample,
            memory_buffer,
            enum_transforms,
            source_reader,
        ] = <[_; 6]>::try_from(resolved).map_err(|_| "invalid Media Foundation export table")?;
        // SAFETY: every export above is mandatory and its ABI matches the Windows SDK.
        unsafe {
            Ok(Self {
                startup: std::mem::transmute::<unsafe extern "system" fn() -> isize, Startup>(
                    startup,
                ),
                media_type: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateObject>(
                    media_type,
                ),
                sample: std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateObject>(
                    sample,
                ),
                memory_buffer: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    CreateBuffer,
                >(memory_buffer),
                enum_transforms: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    EnumTransforms,
                >(enum_transforms),
                source_reader: std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    CreateReader,
                >(source_reader),
                _libraries: [platform, reader],
            })
        }
    }
}

fn api() -> AppResult<&'static Api> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(Api::load)
        .as_ref()
        .map_err(|detail| AppError::Unsupported(format!("Media Foundation 不可用：{detail}")))
}

pub(super) fn ensure_available() -> AppResult<()> {
    api().map(|_| ())
}

pub(super) fn startup(version: u32) -> AppResult<()> {
    let api = api()?;
    // SAFETY: the loaded API outlives every MF session and its exports.
    hr(unsafe { (api.startup)(version, 0).ok() }, "初始化")
}

pub(super) fn media_type() -> AppResult<IMFMediaType> {
    let api = api()?;
    let mut result = std::ptr::null_mut();
    // SAFETY: the SDK writes an owned interface into this out pointer.
    hr(
        unsafe { (api.media_type)(&mut result).and_then(|| Type::from_abi(result)) },
        "建媒体类型",
    )
}

pub(super) fn sample() -> AppResult<IMFSample> {
    let api = api()?;
    let mut result = std::ptr::null_mut();
    // SAFETY: as media_type; the interface is transferred only on success.
    hr(
        unsafe { (api.sample)(&mut result).and_then(|| Type::from_abi(result)) },
        "建样本",
    )
}

pub(super) fn memory_buffer(length: u32) -> AppResult<IMFMediaBuffer> {
    let api = api()?;
    let mut result = std::ptr::null_mut();
    // SAFETY: the SDK receives a valid size and an owned interface out pointer.
    hr(
        unsafe { (api.memory_buffer)(length, &mut result).and_then(|| Type::from_abi(result)) },
        "建样本缓冲",
    )
}

pub(super) fn source_reader(url: PCWSTR) -> AppResult<IMFSourceReader> {
    let api = api()?;
    let mut result = std::ptr::null_mut();
    // SAFETY: the caller's NUL-terminated URL remains alive throughout this call.
    hr(
        unsafe {
            (api.source_reader)(url, std::ptr::null_mut(), &mut result)
                .and_then(|| Type::from_abi(result))
        },
        "建 SourceReader",
    )
}

pub(super) unsafe fn enum_transforms(
    category: GUID,
    flags: u32,
    input: *const MFT_REGISTER_TYPE_INFO,
    output: *const MFT_REGISTER_TYPE_INFO,
    transforms: *mut *mut Option<IMFActivate>,
    count: *mut u32,
) -> AppResult<()> {
    let api = api()?;
    // SAFETY: the caller supplies the SDK-defined input and output pointer lifetimes.
    hr(
        unsafe { (api.enum_transforms)(category, flags, input, output, transforms, count).ok() },
        "MFTEnumEx",
    )
}
