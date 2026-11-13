#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use apple::register;
#[cfg(target_os = "windows")]
pub(crate) use windows::register;
