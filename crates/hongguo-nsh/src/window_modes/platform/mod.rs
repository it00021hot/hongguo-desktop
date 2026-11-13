#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "ios")]
pub(crate) use ios::register;
#[cfg(target_os = "macos")]
pub(crate) use macos::register;
#[cfg(target_os = "windows")]
pub(crate) use windows::register;
