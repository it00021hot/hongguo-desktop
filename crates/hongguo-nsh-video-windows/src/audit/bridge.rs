//! Development-only D3D12VA -> same-adapter D3D11 planar surface bridge.
//! No media pixels are mapped or downloaded. The decoder lease also covers
//! queued copies that have not yet reached the UI (including cancellation).
use hongguo_codec_ffi::VideoFrame;
use hongguo_media_types::{FrameDescriptor, HardwareFrame, HardwareHandle, HardwareKind};
use std::{
    mem::ManuallyDrop,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE},
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL_11_0,
            Direct3D11::{
                ID3D11Device, ID3D11Device1, ID3D11Device5, ID3D11DeviceContext4, ID3D11Fence,
                ID3D11Texture2D,
            },
            Direct3D12::{
                D3D12_COMMAND_LIST_TYPE_COPY, D3D12_COMMAND_QUEUE_DESC, D3D12_FENCE_FLAG_SHARED,
                D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT,
                D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS, D3D12_RESOURCE_STATE_COMMON,
                D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
                D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, D3D12_TEXTURE_LAYOUT_UNKNOWN,
                D3D12CreateDevice, ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue,
                ID3D12Device, ID3D12Fence, ID3D12GraphicsCommandList, ID3D12Resource,
            },
            Dxgi::{
                Common::{DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_P010},
                IDXGIDevice,
            },
        },
    },
    core::Interface,
};

const SLOTS: usize = 6;
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct Slot {
    resource: ID3D12Resource,
    texture: ID3D11Texture2D,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    busy: AtomicBool,
    timer: Option<super::copy_timer::Timer>,
}

pub(super) struct CopySample {
    pub bytes: u64,
    pub submit_us: u64,
    pub gpu_waits: u64,
    pub timing: Arc<Mutex<Option<f64>>>,
}
type CopyOutput = (Arc<dyn HardwareFrame>, CopySample);

struct CopiedFrame {
    _source: Arc<VideoFrame>,
    slot: Arc<Slot>,
    descriptor: FrameDescriptor,
    fence_value: u64,
    timing: Arc<Mutex<Option<f64>>>,
}
// SAFETY: the retained AVFrame and slot own their native objects, and the shared
// D3D11 fence wait is queued before the UI may sample this handle.
unsafe impl HardwareFrame for CopiedFrame {
    fn handle(&self) -> HardwareHandle {
        HardwareHandle {
            kind: HardwareKind::D3d11,
            object: self.slot.texture.as_raw() as usize,
            subresource: 0,
        }
    }
    fn descriptor(&self) -> FrameDescriptor {
        self.descriptor.clone()
    }
}
impl Drop for CopiedFrame {
    fn drop(&mut self) {
        self.slot.busy.store(false, Ordering::Release);
    }
}

pub(super) struct Bridge {
    device: ID3D12Device,
    device11: ID3D11Device,
    queue: ID3D12CommandQueue,
    context11: ID3D11DeviceContext4,
    fence: ID3D12Fence,
    fence11: ID3D11Fence,
    serial: u64,
    shape: Option<(u64, u32, DXGI_FORMAT)>,
    slots: Vec<Arc<Slot>>,
    inflight: Vec<Arc<CopiedFrame>>,
    failed_completion: bool,
}

impl Bridge {
    pub fn new(device11: &ID3D11Device) -> Result<Self, String> {
        unsafe {
            let adapter = device11
                .cast::<IDXGIDevice>()
                .map_err(error)?
                .GetAdapter()
                .map_err(error)?;
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device).map_err(error)?;
            let device = device.ok_or("no D3D12 device")?;
            let expected = adapter.GetDesc().map_err(error)?.AdapterLuid;
            let actual = device.GetAdapterLuid();
            if (expected.HighPart, expected.LowPart) != (actual.HighPart, actual.LowPart) {
                return Err("D3D12 and NSH device adapter LUIDs differ".into());
            }
            let queue: ID3D12CommandQueue = device
                .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
                    Type: D3D12_COMMAND_LIST_TYPE_COPY,
                    ..Default::default()
                })
                .map_err(error)?;
            let fence: ID3D12Fence = device
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(error)?;
            let handle = Handle(
                device
                    .CreateSharedHandle(&fence, None, GENERIC_ALL.0, None)
                    .map_err(error)?,
            );
            let mut fence11 = None;
            device11
                .cast::<ID3D11Device5>()
                .map_err(error)?
                .OpenSharedFence(handle.0, &mut fence11)
                .map_err(error)?;
            Ok(Self {
                context11: device11
                    .GetImmediateContext()
                    .map_err(error)?
                    .cast()
                    .map_err(error)?,
                device11: device11.clone(),
                device,
                queue,
                fence,
                fence11: fence11.ok_or("no D3D11 shared fence")?,
                serial: 0,
                shape: None,
                slots: Vec::new(),
                inflight: Vec::new(),
                failed_completion: false,
            })
        }
    }

    pub fn device(&self) -> usize {
        self.device.as_raw() as usize
    }

    pub fn copy(
        &mut self,
        frame: Arc<VideoFrame>,
        descriptor: FrameDescriptor,
    ) -> Result<Option<CopyOutput>, String> {
        self.reap();
        let handle = frame
            .d3d12_handle()
            .map_err(error)?
            .ok_or("decoder did not produce a D3D12VA frame")?;
        let pointer = handle.texture as *mut std::ffi::c_void;
        let fence_pointer = handle.fence as *mut std::ffi::c_void;
        let source = unsafe { ID3D12Resource::from_raw_borrowed(&pointer) }
            .ok_or("null D3D12 frame resource")?;
        let decoded = unsafe { ID3D12Fence::from_raw_borrowed(&fence_pointer) }
            .ok_or("null D3D12 decode fence")?;
        let desc = unsafe { source.GetDesc() };
        let mut owner: Option<ID3D12Device> = None;
        unsafe { source.GetDevice(&mut owner) }.map_err(error)?;
        if owner.as_ref() != Some(&self.device)
            || desc.MipLevels != 1
            || handle.subresource >= u32::from(desc.DepthOrArraySize)
            || desc.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS.0 == 0
            || !matches!(
                (desc.Format, descriptor.bit_depth),
                (DXGI_FORMAT_NV12, 8) | (DXGI_FORMAT_P010, 10)
            )
        {
            return Err(
                "D3D12VA resource/device/bit-depth or simultaneous-read contract is unsupported"
                    .into(),
            );
        }
        let shape = (desc.Width, desc.Height, desc.Format);
        if self.shape != Some(shape) {
            // Old frames own their old pool until both queues retire it. Waiting
            // for the current displayed frame here would prevent publishing the
            // replacement resolution which is needed to release that frame.
            self.slots = (0..SLOTS)
                .map(|_| self.slot(&desc))
                .collect::<Result<_, _>>()?;
            self.shape = Some(shape);
        }
        let Some(slot) = self
            .slots
            .iter()
            .find(|slot| {
                slot.busy
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            })
            .cloned()
        else {
            return Ok(None);
        };
        let start = Instant::now();
        let result = (|| {
            self.serial = self
                .serial
                .checked_add(1)
                .ok_or("copy fence counter exhausted")?;
            unsafe {
                slot.allocator.Reset().map_err(error)?;
                slot.list.Reset(&slot.allocator, None).map_err(error)?;
                if let Some(timer) = &slot.timer {
                    timer.begin(&slot.list);
                }
                // Both video planes are separate D3D12 subresources. COMMON is
                // promoted implicitly for these simultaneous-access read/copy
                // textures and decays when the COPY batch finishes.
                for plane in 0..2 {
                    let mut from = location(
                        source,
                        handle.subresource + plane * u32::from(desc.DepthOrArraySize),
                    );
                    let mut to = location(&slot.resource, plane);
                    slot.list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
                    ManuallyDrop::drop(&mut from.pResource);
                    ManuallyDrop::drop(&mut to.pResource);
                }
                if let Some(timer) = &slot.timer {
                    timer.end(&slot.list);
                }
                slot.list.Close().map_err(error)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            slot.busy.store(false, Ordering::Release);
            return Err(error);
        }
        let copied = Arc::new(CopiedFrame {
            _source: frame,
            slot,
            descriptor,
            fence_value: self.serial,
            timing: Arc::new(Mutex::new(None)),
        });
        let list: ID3D12CommandList = copied.slot.list.cast().map_err(error)?;
        unsafe { self.queue.Wait(decoded, handle.fence_value) }.map_err(error)?;
        // Once queued, cancellation also retains this pool slice and allocator.
        self.inflight.push(copied.clone());
        unsafe {
            self.queue.ExecuteCommandLists(&[Some(list)]);
        }
        if let Err(error) = unsafe { self.queue.Signal(&self.fence, self.serial) } {
            self.failed_completion = true;
            return Err(error.to_string());
        }
        unsafe { self.context11.Wait(&self.fence11, self.serial) }.map_err(error)?;
        let bytes = desc
            .Width
            .checked_mul(u64::from(desc.Height))
            .and_then(|pixels| {
                pixels.checked_mul(3).map(|bytes| {
                    if desc.Format == DXGI_FORMAT_P010 {
                        bytes
                    } else {
                        bytes / 2
                    }
                })
            })
            .ok_or("copy byte count overflow")?;
        let timing = copied.timing.clone();
        Ok(Some((
            copied,
            CopySample {
                bytes,
                submit_us: start.elapsed().as_micros() as u64,
                gpu_waits: 2,
                timing,
            },
        )))
    }

    fn slot(&self, source: &D3D12_RESOURCE_DESC) -> Result<Arc<Slot>, String> {
        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            self.device
                .CreateCommittedResource(
                    &D3D12_HEAP_PROPERTIES {
                        Type: D3D12_HEAP_TYPE_DEFAULT,
                        CreationNodeMask: 1,
                        VisibleNodeMask: 1,
                        ..Default::default()
                    },
                    D3D12_HEAP_FLAG_SHARED,
                    &D3D12_RESOURCE_DESC {
                        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                        Width: source.Width,
                        Height: source.Height,
                        DepthOrArraySize: 1,
                        MipLevels: 1,
                        Format: source.Format,
                        SampleDesc: source.SampleDesc,
                        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                        Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
                        ..Default::default()
                    },
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
                .map_err(error)?;
            let resource = resource.ok_or("no shared D3D12 planar texture")?;
            let handle = Handle(
                self.device
                    .CreateSharedHandle(&resource, None, GENERIC_ALL.0, None)
                    .map_err(error)?,
            );
            let texture: ID3D11Texture2D = self
                .device11
                .cast::<ID3D11Device1>()
                .map_err(error)?
                .OpenSharedResource1(handle.0)
                .map_err(error)?;
            let allocator: ID3D12CommandAllocator = self
                .device
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COPY)
                .map_err(error)?;
            let list: ID3D12GraphicsCommandList = self
                .device
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_COPY, &allocator, None)
                .map_err(error)?;
            list.Close().map_err(error)?;
            Ok(Arc::new(Slot {
                resource,
                texture,
                allocator,
                list,
                busy: AtomicBool::new(false),
                timer: super::copy_timer::Timer::create(&self.device, &self.queue)?,
            }))
        }
    }

    fn reap(&mut self) {
        let completed = unsafe { self.fence.GetCompletedValue() };
        self.inflight.retain(|frame| {
            if frame.fence_value > completed {
                return true;
            }
            if completed != u64::MAX
                && let Some(timer) = &frame.slot.timer
            {
                match timer.read_us() {
                    Ok(us) => {
                        *frame
                            .timing
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(us)
                    }
                    Err(error) => log::warn!("copy timestamp unavailable: {error}"),
                }
            }
            false
        });
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if self.failed_completion && unsafe { self.device.GetDeviceRemovedReason() }.is_ok() {
            // A failed post-submit Signal leaves no provable completion point.
            // This diagnostic exits with an error; keep leases until process
            // exit instead of reusing GPU memory with unknown completion.
            std::mem::forget((
                std::mem::take(&mut self.inflight),
                self.queue.clone(),
                self.fence.clone(),
                self.device.clone(),
            ));
            return;
        }
        // The decode worker owns Bridge. Closing the UI never releases an AVFrame
        // before its already submitted copy; this shutdown-only wait is excluded
        // from steady-state timings and never downloads pixel data.
        while !self.inflight.is_empty() {
            self.reap();
            if unsafe { self.device.GetDeviceRemovedReason() }.is_err() {
                break;
            }
            if !self.inflight.is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn location(resource: &ID3D12Resource, index: u32) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(resource.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: index,
        },
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
