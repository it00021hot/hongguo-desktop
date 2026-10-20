//! `AVAssetResourceLoader` 供数：把一集的明文按 offset/length 喂给 AVFoundation。
//!
//! 两种数据源：
//! - [`Source::Stream`]：生产形态——`hongguo-stream` 的 `(vid, 档位)` 渐进流，
//!   按需等待密文区间、惰性解密；`contentLength` 用 `plain_len`。
//! - [`Source::Bytes`]：Phase 0 的整段内存样片，用于对照「全量字节」与
//!   「渐进供数」两种模式的切集空档。
//!
//! 关键语义：
//! - `AVAssetResourceLoader.delegate` 是 **weak**，delegate 必须由调用方强引用；
//! - `contentType` 是 UTI（`public.mpeg-4`），且必须落在
//!   `contentInformationRequest.allowedContentTypes` 内，否则 `finishLoading`
//!   抛 ObjC 异常（Rust 侧无法 catch，会 abort）；
//! - 必须声明 `byteRangeAccessSupported=YES` + `contentLength`，否则 seek 失败；
//! - 每个请求一个取消位：`didCancelLoadingRequest:` 与 `shouldWait...` 必须在
//!   不同线程上跑（这里用并发 dispatch queue），否则 seek/切集时取消永远
//!   送不进来，旧读取会把串行队列堵满 30s。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hongguo_stream::{StreamReadError, read_range_cancellable, total_len};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSData, NSString};

/// 一次 `read_range_cancellable` 最多取的字节数：渐进流可能只到了头部，
/// 分块读取既能及时响应取消，也避免一次分配整集明文。
const READ_CHUNK_BYTES: u64 = 4 * 1024 * 1024;
/// 渐进流注册（`plain_len` 可见）前 `contentInformationRequest` 的最长等待。
const TOTAL_LEN_TIMEOUT: Duration = Duration::from_secs(5);
/// `dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0)` 的 qos 常量。
const QOS_CLASS_DEFAULT: isize = 0x15;

pub static REQUESTS: AtomicU64 = AtomicU64::new(0);
pub static BYTES_SERVED: AtomicU64 = AtomicU64::new(0);
pub static ERRORS: AtomicU64 = AtomicU64::new(0);

#[link(name = "System")]
unsafe extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut AnyObject;
}

/// 一集的数据来源。
#[derive(Clone)]
pub enum Source {
    /// 整段明文在内存（Phase 0 对照模式）。
    Bytes(Arc<[u8]>),
    /// hongguo-stream 渐进流。
    Stream { vid: String, definition: u32 },
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Self::Bytes(bytes) => format!("bytes({}B)", bytes.len()),
            Self::Stream { vid, definition } => format!("stream({vid}@{definition})"),
        }
    }
}

struct LoaderIvars {
    source: Source,
    /// 正在处理中的请求 → 取消位。key 用请求对象地址。
    /// `Arc` 是因为处理线程结束时要自己摘掉登记。
    cancelled: Arc<Mutex<HashMap<usize, Arc<AtomicBool>>>>,
}

/// 把请求对象交给处理线程。`Retained` 本身 !Send，但 AVFoundation 明确允许
/// 异步回包；线程持有强引用，finish/cancel 前对象不会释放。
struct SendRequest(Retained<AnyObject>);

// SAFETY: 见 [`SendRequest`] 注释；只在 loader 线程内调用该对象的公开方法。
unsafe impl Send for SendRequest {}

impl SendRequest {
    /// 用方法取引用，避免闭包按字段捕获 `Retained`（那样会绕过本包装的 Send）。
    fn get(&self) -> &AnyObject {
        &self.0
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "HongguoAvSpikeResourceLoader"]
    #[ivars = LoaderIvars]
    struct Loader;

    impl Loader {
        #[unsafe(method(resourceLoader:shouldWaitForLoadingOfRequestedResource:))]
        fn should_wait(&self, _loader: &AnyObject, request: &AnyObject) -> bool {
            let key = request as *const AnyObject as usize;
            let cancelled = Arc::new(AtomicBool::new(false));
            let flags = Arc::clone(&self.ivars().cancelled);
            flags
                .lock()
                .expect("取消位表 poisoned")
                .insert(key, Arc::clone(&cancelled));
            // SAFETY: request 在回调期间有效；retain 后由处理线程持有。
            let Some(request) = (unsafe {
                Retained::retain(request as *const AnyObject as *mut AnyObject)
            }) else {
                return false.into();
            };
            let source = self.ivars().source.clone();
            let request = SendRequest(request);
            // 立即返回，把回包放到独立线程：AVFoundation 要等 shouldWait 返回
            // 才会消费已经 respond 的字节；在回调里阻塞到整段读完会让渐进流
            // 永远起不来（本 spike 实测）。
            std::thread::Builder::new()
                .name("hongguo-av-loader".to_owned())
                .spawn(move || {
                    serve(source, request.get(), &cancelled);
                    flags
                        .lock()
                        .expect("取消位表 poisoned")
                        .remove(&key);
                })
                .expect("loader 线程创建失败");
            true
        }

        #[unsafe(method(resourceLoader:didCancelLoadingRequest:))]
        fn did_cancel(&self, _loader: &AnyObject, request: &AnyObject) {
            let key = request as *const AnyObject as usize;
            if let Some(cancelled) = self
                .ivars()
                .cancelled
                .lock()
                .expect("取消位表 poisoned")
                .get(&key)
            {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
    }
);

/// 给 asset 挂上 delegate。
///
/// `AVAssetResourceLoader.delegate` 是 weak：这里把 delegate 泄漏成进程级
/// 强引用（spike 一次性进程，退出即回收），调用方不需要持有句柄。
pub fn attach(asset: &AnyObject, source: Source) {
    let ivars = LoaderIvars {
        source,
        cancelled: Arc::new(Mutex::new(HashMap::new())),
    };
    // SAFETY: NSObject 子类，ivars 已设置；init 由 NSObject 提供。
    let loader: Retained<Loader> =
        unsafe { msg_send![super(Loader::alloc().set_ivars(ivars)), init] };
    // SAFETY: 全局并发队列常驻；用并发而不是串行，didCancel 才能与阻塞中的
    // shouldWait 并行执行。
    let queue = unsafe { dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0) };
    // SAFETY: resourceLoader 恒非空；delegate 与 queue 的类型来自公开 API。
    let resource_loader: *mut AnyObject = unsafe { msg_send![asset, resourceLoader] };
    let _: () = unsafe {
        msg_send![
            resource_loader,
            setDelegate: &*loader,
            queue: queue
        ]
    };
    std::mem::forget(loader);
}

fn serve(source: Source, request: &AnyObject, cancelled: &AtomicBool) {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    let Some(total) = total_bytes(&source, cancelled) else {
        if cancelled.load(Ordering::SeqCst) {
            // 请求已取消：不 finish，也不报错。
            return;
        }
        log::error!("[AvSpike] {} 拿不到 contentLength", source.label());
        finish_with_error(request, -1, "stream total length unavailable");
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return;
    };

    // 1) 内容信息：UTI + 总长 + 支持随机 Range（seek 的前提）。
    // SAFETY: 属性可能为 nil，逐个判空。
    let content_info: *mut AnyObject = unsafe { msg_send![request, contentInformationRequest] };
    if !content_info.is_null() {
        let content_type = pick_content_type(content_info);
        let _: () = unsafe { msg_send![content_info, setContentLength: total as i64] };
        let _: () = unsafe { msg_send![content_info, setContentType: &*content_type] };
        let _: () = unsafe { msg_send![content_info, setByteRangeAccessSupported: true] };
    }

    // 2) 数据段：offset/length（不是 HTTP Range 头）。
    // SAFETY: dataRequest 可能为 nil（纯内容信息查询）。
    let data_request: *mut AnyObject = unsafe { msg_send![request, dataRequest] };
    if !data_request.is_null() {
        let offset: usize = unsafe { msg_send![data_request, requestedOffset] };
        let length: usize = unsafe { msg_send![data_request, requestedLength] };
        let all_to_end: bool = unsafe { msg_send![data_request, requestsAllDataToEndOfResource] };
        let start = (offset as u64).min(total);
        let end = if all_to_end || length == usize::MAX {
            total
        } else {
            start.saturating_add(length as u64).min(total)
        };
        log::debug!(
            "[AvSpike][loader] {} request offset={} len={} all_to_end={all_to_end} total={total} -> {start}..{end}",
            source.label(),
            offset,
            length,
        );
        // SAFETY: 上面已判空，dataRequest 在本次请求生命周期内有效。
        let data_request = unsafe { &*data_request };
        if !respond_range(&source, request, data_request, start, end, cancelled) {
            return;
        }
    }

    // SAFETY: 一次请求只 finish 一次。
    let _: () = unsafe { msg_send![request, finishLoading] };
}

/// 按数据源回一段；返回 `false` 表示已取消或已 `finishLoadingWithError`。
fn respond_range(
    source: &Source,
    request: &AnyObject,
    data_request: &AnyObject,
    start: u64,
    end: u64,
    cancelled: &AtomicBool,
) -> bool {
    match source {
        Source::Bytes(bytes) => {
            let start_index = (start as usize).min(bytes.len());
            let end_index = (end as usize).min(bytes.len());
            respond_with_data(data_request, &bytes[start_index..end_index]);
            true
        }
        Source::Stream { vid, definition } => {
            let mut position = start;
            while position < end {
                if cancelled.load(Ordering::SeqCst) {
                    return false;
                }
                let want = (end - position).min(READ_CHUNK_BYTES);
                let result = read_range_cancellable(vid, *definition, position, Some(want), || {
                    cancelled.load(Ordering::SeqCst)
                });
                match result {
                    Ok(chunk) if !chunk.bytes.is_empty() => {
                        respond_with_data(data_request, &chunk.bytes);
                        position += chunk.bytes.len() as u64;
                    }
                    Ok(_) => {
                        log::warn!(
                            "[AvSpike] {vid}@{definition} 在 {position} 处读到空段（total={end}）"
                        );
                        break;
                    }
                    Err(StreamReadError::Cancelled) => return false,
                    Err(error) => {
                        log::error!(
                            "[AvSpike] {vid}@{definition} 读取 {position}..{} 失败: {error}",
                            position + want
                        );
                        finish_with_error(request, -2, &error.to_string());
                        ERRORS.fetch_add(1, Ordering::Relaxed);
                        return false;
                    }
                }
            }
            true
        }
    }
}

fn respond_with_data(request: &AnyObject, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let data = NSData::with_bytes(bytes);
    // SAFETY: data 是本次调用内有效的局部对象。
    let _: () = unsafe { msg_send![request, respondWithData: &*data] };
    BYTES_SERVED.fetch_add(bytes.len() as u64, Ordering::Relaxed);
}

/// 等这一档的明文总长。渐进流注册后立即可得；整集模式要等落盘。
fn total_bytes(source: &Source, cancelled: &AtomicBool) -> Option<u64> {
    match source {
        Source::Bytes(bytes) => Some(bytes.len() as u64),
        Source::Stream { vid, definition } => {
            let deadline = Instant::now() + TOTAL_LEN_TIMEOUT;
            loop {
                if cancelled.load(Ordering::SeqCst) {
                    return None;
                }
                if let Some(total) = total_len(vid, *definition) {
                    return Some(total);
                }
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn finish_with_error(request: &AnyObject, code: isize, message: &str) {
    let domain = NSString::from_str("HongguoAvSpike");
    let description = NSString::from_str(message);
    let key = NSString::from_str("NSLocalizedDescription");
    // 用 NSDictionary 的 +dictionaryWithObject:forKey: 组 userInfo（避免为
    // spike 再开 NSDictionary feature 的额外绑定面）。
    let dictionary_class = objc2::runtime::AnyClass::get(c"NSDictionary").expect("NSDictionary");
    // SAFETY: 公开类方法；两个对象均在本调用内有效。
    let user_info: Retained<AnyObject> = unsafe {
        msg_send![
            dictionary_class,
            dictionaryWithObject: &*description,
            forKey: &*key
        ]
    };
    let error_class = objc2::runtime::AnyClass::get(c"NSError").expect("NSError");
    // SAFETY: 公开类方法，userInfo 非空。
    let error: Retained<AnyObject> = unsafe {
        msg_send![
            error_class,
            errorWithDomain: &*domain,
            code: code,
            userInfo: &*user_info
        ]
    };
    // SAFETY: NSError 对象有效；该方法结束请求。
    let _: () = unsafe { msg_send![request, finishLoadingWithError: &*error] };
}

/// `contentType` 是 UTI 而不是 MIME；系统给了 allowedContentTypes 就必须落
/// 在其内，否则 `finishLoading` 抛 ObjC 异常。
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
