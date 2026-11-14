//! Timestamp-only readback; no media pixel resource is ever CPU mapped.
use std::ffi::c_void;
use windows::Win32::Graphics::{
    Direct3D12::*,
    Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC},
};

pub(super) struct Timer {
    heap: ID3D12QueryHeap,
    readback: ID3D12Resource,
    frequency: u64,
}
impl Timer {
    pub fn create(
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
    ) -> Result<Option<Self>, String> {
        let mut support = D3D12_FEATURE_DATA_D3D12_OPTIONS3::default();
        let supported = unsafe {
            device.CheckFeatureSupport(
                D3D12_FEATURE_D3D12_OPTIONS3,
                (&raw mut support).cast(),
                size_of::<D3D12_FEATURE_DATA_D3D12_OPTIONS3>() as u32,
            )
        }
        .is_ok();
        if !supported {
            return Ok(None);
        }
        if !support.CopyQueueTimestampQueriesSupported.as_bool() {
            return Ok(None);
        }
        unsafe {
            let mut heap: Option<ID3D12QueryHeap> = None;
            device
                .CreateQueryHeap(
                    &D3D12_QUERY_HEAP_DESC {
                        Type: D3D12_QUERY_HEAP_TYPE_COPY_QUEUE_TIMESTAMP,
                        Count: 2,
                        ..Default::default()
                    },
                    &mut heap,
                )
                .map_err(error)?;
            let heap = heap.ok_or("no copy timestamp query heap")?;
            let mut readback: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(
                    &D3D12_HEAP_PROPERTIES {
                        Type: D3D12_HEAP_TYPE_READBACK,
                        ..Default::default()
                    },
                    D3D12_HEAP_FLAG_NONE,
                    &D3D12_RESOURCE_DESC {
                        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                        Width: 16,
                        Height: 1,
                        DepthOrArraySize: 1,
                        MipLevels: 1,
                        Format: DXGI_FORMAT_UNKNOWN,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                        ..Default::default()
                    },
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    None,
                    &mut readback,
                )
                .map_err(error)?;
            Ok(Some(Self {
                heap,
                readback: readback.ok_or("no timestamp readback buffer")?,
                frequency: queue.GetTimestampFrequency().map_err(error)?,
            }))
        }
    }
    pub fn begin(&self, list: &ID3D12GraphicsCommandList) {
        unsafe {
            list.EndQuery(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, 0);
        }
    }
    pub fn end(&self, list: &ID3D12GraphicsCommandList) {
        unsafe {
            list.EndQuery(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, 1);
            list.ResolveQueryData(
                &self.heap,
                D3D12_QUERY_TYPE_TIMESTAMP,
                0,
                2,
                &self.readback,
                0,
            );
        }
    }
    /// The caller has already observed its COPY completion fence.
    pub fn read_us(&self) -> Result<f64, String> {
        unsafe {
            let mut pointer: *mut c_void = std::ptr::null_mut();
            self.readback
                .Map(
                    0,
                    Some(&D3D12_RANGE { Begin: 0, End: 16 }),
                    Some(&mut pointer),
                )
                .map_err(error)?;
            if pointer.is_null() {
                self.readback.Unmap(0, None);
                return Err("null timestamp mapping".into());
            }
            let ticks = std::slice::from_raw_parts(pointer.cast::<u64>(), 2);
            let delta = ticks[1].checked_sub(ticks[0]);
            self.readback
                .Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
            let delta = delta.ok_or("copy timestamp counter moved backwards")?;
            if self.frequency == 0 {
                return Err("zero copy queue timestamp frequency".into());
            }
            Ok(delta as f64 * 1_000_000.0 / self.frequency as f64)
        }
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
