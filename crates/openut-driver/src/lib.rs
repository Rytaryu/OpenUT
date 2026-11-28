//! A SteamVR server driver that receives poses from the local OpenUT process.

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
#[allow(unsafe_code)]
mod abi;

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
mod frame;
pub mod gate;
pub mod pose;
pub mod receiver;
pub mod slots;
pub mod table;
