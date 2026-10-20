//! `AVAssetResourceLoader` 供数：把整段 fMP4 字节按 Range 喂给 AVFoundation。
//!
//! 这是 Phase 1 真正要落地的形态——播放器不认 `hongguo-stream://`，只认
//! 自定义 scheme 的 `AVURLAsset`，由 delegate 回答「内容类型/总长/是否支持
//! Range」并把请求的字节段交回去。spike 用整段内存字节模拟「解密后的明文」，
//! 请求语义与 `hongguo-stream` 的 Range 应答一一对应。
//!
//! 生命周期：`AVAssetResourceLoader.delegate` 是 **weak**，delegate 必须由
//! 调用方强引用；这里按 spike 的进程级生命周期直接泄漏一个实例。

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{ClassType, define_class, msg_send};
use objc2_foundation::{NSData, NSString};

static SAMPLE: OnceLock<Vec<u8>> = OnceLock::new();

pub static REQUESTS: AtomicU64 = AtomicU64::new(0);
pub static BYTES_SERVED: AtomicU64 = AtomicU64::new(0);

#[link(name = "System")]
unsafe extern "C" {
    fn dispatch_queue_create(label: *const i8, attr: *const std::ffi::c_void) -> *mut AnyObject;
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "HongguoAvSpikeResourceLoader"]
    struct Loader;

    impl Loader {
        #[unsafe(method(resourceLoader:shouldWaitForLoadingOfRequestedResource:))]
        fn should_wait(&self, _loader: &AnyObject, request: &AnyObject) -> bool {
            serve(request);
            true
        }

        #[unsafe(method(resourceLoader:didCancelLoadingRequest:))]
        fn did_cancel(&self, _loader: &AnyObject, _request: &AnyObject) {
            // spike 的内存供数是同步完成的，没有可取消的在途状态；生产版本
            // 要在这里把 serve_cancellable 的 cancelled 标志置位。
        }
    }
);

impl Loader {
    fn new() -> Retained<Self> {
        // SAFETY: NSObject 的 +new；类已由 define_class! 注册。
        unsafe { msg_send![Self::class(), new] }
    }
}

/// 设置进程级样片字节（只允许一次；spike 进程内所有 asset 共用）。
pub fn set_bytes(bytes: Vec<u8>) {
    if SAMPLE.set(bytes).is_err() {
        log::warn!("[AvSpike] 样片字节重复设置，忽略");
    }
}

/// 给 asset 挂上 delegate（每个 asset 一个），并保证 delegate 存活。
pub fn attach(asset: &AnyObject) {
    let loader = Loader::new();
    // SAFETY: 串行队列，delegate 回调在该队列上执行；queue 由 dispatch 管理，
    // 进程内泄漏不回收。
    let queue =
        unsafe { dispatch_queue_create(c"hongguo.avspike.loader".as_ptr(), std::ptr::null()) };
    // SAFETY: resourceLoader 恒非空；setDelegate:queue: 的参数类型为 id + dispatch_queue_t。
    let resource_loader: *mut AnyObject = unsafe { msg_send![asset, resourceLoader] };
    let _: () = unsafe {
        msg_send![
            resource_loader,
            setDelegate: &*loader,
            queue: queue.cast::<AnyObject>()
        ]
    };
    // delegate 是 weak：泄漏一个强引用，保证整个 spike 生命周期有效。
    std::mem::forget(loader);
}

fn serve(request: &AnyObject) {
    let Some(bytes) = SAMPLE.get() else {
        log::error!("[AvSpike] 资源请求到达时样片字节尚未设置");
        return;
    };

    // 1) 内容信息：UTI、总长、支持随机 Range（缺一不可，否则播放/seek 失败）。
    // SAFETY: 两个属性都可能为 nil，逐个判空。
    let content_info: *mut AnyObject = unsafe { msg_send![request, contentInformationRequest] };
    if !content_info.is_null() {
        let content_type = pick_content_type(content_info);
        let _: () = unsafe { msg_send![content_info, setContentLength: bytes.len() as i64] };
        let _: () = unsafe { msg_send![content_info, setContentType: &*content_type] };
        let _: () = unsafe { msg_send![content_info, setByteRangeAccessSupported: true] };
    }

    // 2) 数据段：AVFoundation 给的是 offset/length（不是 HTTP Range 头）。
    // SAFETY: dataRequest 可能为 nil（纯内容信息查询）。
    let data_request: *mut AnyObject = unsafe { msg_send![request, dataRequest] };
    if !data_request.is_null() {
        let offset: usize = unsafe { msg_send![data_request, requestedOffset] };
        let length: usize = unsafe { msg_send![data_request, requestedLength] };
        let all_to_end: bool = unsafe { msg_send![data_request, requestsAllDataToEndOfResource] };
        let start = offset.min(bytes.len());
        let end = if all_to_end || length == usize::MAX {
            bytes.len()
        } else {
            start.saturating_add(length).min(bytes.len())
        };
        let data = NSData::with_bytes(&bytes[start..end]);
        let _: () = unsafe { msg_send![data_request, respondWithData: &*data] };
        BYTES_SERVED.fetch_add((end - start) as u64, Ordering::Relaxed);
    }

    REQUESTS.fetch_add(1, Ordering::Relaxed);
    // SAFETY: 一次请求只 finish 一次。
    let _: () = unsafe { msg_send![request, finishLoading] };
}

/// `contentType` 是 UTI 而不是 MIME；系统给了 allowedContentTypes 就必须落
/// 在其内，否则 `finishLoading` 抛 ObjC 异常（Rust 侧无法 catch）。
fn pick_content_type(content_info: *mut AnyObject) -> Retained<NSString> {
    // SAFETY: allowedContentTypes 可能为 nil。
    let allowed: *mut AnyObject = unsafe { msg_send![content_info, allowedContentTypes] };
    if !allowed.is_null() {
        let count: usize = unsafe { msg_send![allowed, count] };
        if count > 0 {
            let first: *mut NSString = unsafe { msg_send![allowed, objectAtIndex: 0usize] };
            if !first.is_null() {
                // SAFETY: 数组持有该字符串；retain 后脱离数组生命周期。
                if let Some(first) = unsafe { Retained::retain(first) } {
                    return first;
                }
            }
        }
    }
    NSString::from_str("public.mpeg-4")
}
