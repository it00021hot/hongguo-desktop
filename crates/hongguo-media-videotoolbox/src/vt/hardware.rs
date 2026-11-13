// iOS 17/17.4 才导出硬件选项符号；较早系统由 VideoToolbox 默认选择后端。
#[cfg(target_os = "ios")]
unsafe fn optional_key(name: &std::ffi::CStr) -> Option<super::ffi::CFStringRef> {
    // SAFETY: VideoToolbox 已由静态框架依赖载入；找到的是 SDK 声明的 CFStringRef 全局变量。
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    if symbol.is_null() {
        None
    } else {
        Some(unsafe { *symbol.cast::<super::ffi::CFStringRef>() })
    }
}

pub(super) unsafe fn encoder_enabled() -> Option<super::ffi::CFStringRef> {
    #[cfg(target_os = "macos")]
    {
        Some(unsafe {
            super::ffi::kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder
        })
    }
    #[cfg(target_os = "ios")]
    {
        unsafe {
            optional_key(c"kVTVideoEncoderSpecification_EnableHardwareAcceleratedVideoEncoder")
        }
    }
}

pub(super) unsafe fn decoder_enabled() -> Option<super::ffi::CFStringRef> {
    #[cfg(target_os = "macos")]
    {
        Some(unsafe {
            super::ffi::kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder
        })
    }
    #[cfg(target_os = "ios")]
    {
        unsafe {
            optional_key(c"kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder")
        }
    }
}

pub(super) unsafe fn encoder_using() -> Option<super::ffi::CFStringRef> {
    #[cfg(target_os = "macos")]
    {
        Some(unsafe { super::ffi::kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder })
    }
    #[cfg(target_os = "ios")]
    {
        unsafe { optional_key(c"kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder") }
    }
}

pub(super) unsafe fn decoder_using() -> Option<super::ffi::CFStringRef> {
    #[cfg(target_os = "macos")]
    {
        Some(unsafe {
            super::ffi::kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder
        })
    }
    #[cfg(target_os = "ios")]
    {
        unsafe { optional_key(c"kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder") }
    }
}
