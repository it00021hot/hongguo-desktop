use std::{ffi::c_void, ptr::NonNull};

use hongguo_common::{AppError, AppResult};
use hongguo_vvdec_sys::vvdecComponentType;

unsafe extern "C" {
    fn malloc_create_zone(start_size: usize, flags: u32) -> *mut c_void;
    fn malloc_destroy_zone(zone: *mut c_void);
    fn malloc_set_zone_name(zone: *mut c_void, name: *const std::ffi::c_char);
    fn malloc_zone_memalign(zone: *mut c_void, alignment: usize, size: usize) -> *mut c_void;
    fn malloc_zone_free(zone: *mut c_void, storage: *mut c_void);
}

pub(super) struct AllocationZone(NonNull<c_void>);

impl AllocationZone {
    pub(super) fn new() -> AppResult<Self> {
        let zone = NonNull::new(unsafe { malloc_create_zone(0, 0) })
            .ok_or_else(|| AppError::Media("VVdeC allocation zone creation failed".into()))?;
        unsafe { malloc_set_zone_name(zone.as_ptr(), c"VVdeC frame buffers".as_ptr()) };
        Ok(Self(zone))
    }

    pub(super) fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for AllocationZone {
    fn drop(&mut self) {
        unsafe { malloc_destroy_zone(self.0.as_ptr()) };
    }
}

pub(super) unsafe extern "C" fn create(
    zone: *mut c_void,
    _component: vvdecComponentType,
    size: u32,
    alignment: u32,
    allocator: *mut *mut c_void,
) -> *mut c_void {
    if zone.is_null() || allocator.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { allocator.write(std::ptr::null_mut()) };
    if size == 0
        || !alignment.is_power_of_two()
        || (alignment as usize) < std::mem::size_of::<*mut c_void>()
    {
        return std::ptr::null_mut();
    }
    let storage = unsafe { malloc_zone_memalign(zone, alignment as usize, size as usize) };
    unsafe { allocator.write(storage) };
    storage
}

pub(super) unsafe extern "C" fn unref(zone: *mut c_void, storage: *mut c_void) {
    unsafe { malloc_zone_free(zone, storage) };
}
