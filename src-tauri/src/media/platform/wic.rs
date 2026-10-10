//! WIC 静图解码（Windows）：HEIC → 32bppBGR → JPEG。
//!
//! 平台级的封面通路。WIC 本体每台 Windows 都有，但 HEIF/HEIC **解码器**
//! 是可选商店扩展（「HEIF 图像扩展」+「HEVC 视频扩展」；OEM 机型常预装，
//! 纯净安装经常没有）——所以与 MF 硬编同一条探测纪律：**运行时试，
//! 不假设存在**。解码器创建失败（典型为 COMPONENTNOTFOUND）就缓存
//! 「不可用」，封面链路掉到 ffmpeg/纯软解；「重新检测」清缓存重试。
//!
//! 管线：内存 IStream → `CreateDecoderFromStream` → 首帧 →
//! FormatConverter 统一到 32bppBGR → `CopyPixels` → 交换 B/R →
//! jpeg-encoder 编码。编码不依赖 WIC 的 JPEG encoder（同样是可选组件），
//! 统一用我们自己的 [`jpeg_encoder`]，与纯软解级（[`crate::media::heif`]）
//! 共享同一产物口径。

use windows::Win32::Foundation::{GlobalFree, WINCODEC_ERR_COMPONENTNOTFOUND};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGR, IWICBitmapDecoder, IWICFormatConverter,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

/// 「本机没有 HEIF 解码器」的探测缓存：false = 还没试过或已重置。
static HEIC_DECODER_MISSING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 清除探测缓存（「重新检测」按钮的钩子，与 MF/VT 同一批次）。
pub fn clear_probe_cache() {
    HEIC_DECODER_MISSING.store(false, std::sync::atomic::Ordering::Release);
}

/// HEIC 字节 → JPEG。本机没有 HEIF 解码器时返回 `None`（调用方掉级），
/// 有解码器但图像解不开时返回 `Err`（这张图换别的路大概率也不行，但
/// 调用方仍会尝试下一级，不赌单个样本）。
pub fn heic_to_jpeg(heic: &[u8]) -> Option<Result<Vec<u8>, String>> {
    if HEIC_DECODER_MISSING.load(std::sync::atomic::Ordering::Acquire) {
        return None;
    }
    match try_decode(heic) {
        Ok(jpg) => Some(Ok(jpg)),
        Err(HeicError::CodecMissing) => {
            HEIC_DECODER_MISSING.store(true, std::sync::atomic::Ordering::Release);
            log::info!("[Cover/WIC] 本机没有 HEIF 解码器，平台级封面长期下线（重新检测可重试）");
            None
        }
        Err(HeicError::Decode(msg)) => Some(Err(msg)),
    }
}

enum HeicError {
    /// 没有可用的 HEIF/HEVC 解码器（可选扩展未装）——机器级，可缓存。
    CodecMissing,
    /// 解码器在，但这份图像解不开——图片级。
    Decode(String),
}

fn try_decode(heic: &[u8]) -> Result<Vec<u8>, HeicError> {
    // worker 线程不是 COM 线程：进 MTA（与 mf 同款）。已初始化过或不匹配
    // 的返回值都不致命，套间仍在，继续用。
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    decode_with_factory(heic)
}

fn decode_with_factory(heic: &[u8]) -> Result<Vec<u8>, HeicError> {
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| HeicError::CodecMissing)?;

        let stream = create_stream(heic)?;
        let decoder: IWICBitmapDecoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .map_err(map_factory_error)?;

        let frame = decoder
            .GetFrame(0)
            .map_err(|e| HeicError::Decode(format!("取解码帧失败: {e}")))?;

        // 统一转 32bppBGR：任何来源像素格式（含 10bit/索引色）都能落到这里
        let converter: IWICFormatConverter = factory
            .CreateFormatConverter()
            .map_err(|e| HeicError::Decode(format!("建格式转换器失败: {e}")))?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| HeicError::Decode(format!("像素格式转换失败: {e}")))?;

        let (mut width, mut height) = (0u32, 0u32);
        converter
            .GetSize(&mut width, &mut height)
            .map_err(|e| HeicError::Decode(format!("读尺寸失败: {e}")))?;
        if width == 0 || height == 0 {
            return Err(HeicError::Decode("图像尺寸为 0".into()));
        }

        let stride = width as usize * 4;
        let mut bgra = vec![0u8; stride * height as usize];
        converter
            .CopyPixels(std::ptr::null(), stride as u32, &mut bgra)
            .map_err(|e| HeicError::Decode(format!("拷贝像素失败: {e}")))?;

        // BGRA → RGB（丢弃 alpha；封面不透明）
        let (w, h) = (width as usize, height as usize);
        let mut rgb = vec![0u8; w * h * 3];
        for px in 0..w * h {
            rgb[px * 3] = bgra[px * 4 + 2];
            rgb[px * 3 + 1] = bgra[px * 4 + 1];
            rgb[px * 3 + 2] = bgra[px * 4];
        }
        encode_jpeg(rgb, w, h)
    }
}

/// 把字节塞进 HGLOBAL 内存流；流接管 HGLOBAL（DeleteOnRelease），失败路径
/// 自己 GlobalFree，不漏。
fn create_stream(bytes: &[u8]) -> Result<windows::Win32::System::Com::IStream, HeicError> {
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|_| HeicError::Decode("分配内存流失败".into()))?;
        let dst = GlobalLock(hglobal);
        if dst.is_null() {
            let _ = GlobalFree(Some(hglobal));
            return Err(HeicError::Decode("锁定内存流失败".into()));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
        let _ = GlobalUnlock(hglobal);
        CreateStreamOnHGlobal(hglobal, true).map_err(|e| {
            let _ = GlobalFree(Some(hglobal));
            HeicError::Decode(format!("建内存流失败: {e}"))
        })
    }
}

fn map_factory_error(e: windows::core::Error) -> HeicError {
    // 组件未找到 = 没装 HEIF/HEVC 扩展；其余错误按「解码失败」处理
    if e.code() == WINCODEC_ERR_COMPONENTNOTFOUND {
        HeicError::CodecMissing
    } else {
        HeicError::Decode(format!("建 HEIC 解码器失败: {e}"))
    }
}

fn encode_jpeg(rgb: Vec<u8>, width: usize, height: usize) -> Result<Vec<u8>, HeicError> {
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, 85);
    enc.encode(
        &rgb,
        width as u16,
        height as u16,
        jpeg_encoder::ColorType::Rgb,
    )
    .map_err(|e| HeicError::Decode(format!("JPEG 编码失败: {e}")))?;
    Ok(out)
}
