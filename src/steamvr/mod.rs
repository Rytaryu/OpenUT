//! SteamVR client thread, raw tracking samples, and dashboard overlay access.

pub mod client;

#[cfg(windows)]
mod ffi;

/// Starts the native SteamVR client. The origin must match the tracker clock.
#[cfg(windows)]
pub fn start(origin: std::time::Instant) -> std::io::Result<client::Client> {
    client::Client::start(origin, ffi::OpenVrBackend::default)
}

#[cfg(all(windows, test))]
#[test]
fn links_the_native_client_without_starting_steamvr() {
    let entry: fn(std::time::Instant) -> std::io::Result<client::Client> = start;
    std::hint::black_box(entry);
}
