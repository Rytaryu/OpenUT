//! OpenUT: an unofficial runtime and tools for VIVE Ultimate Tracker on
//! Windows + SteamVR.
//!
//! This is the smallest crate the checks can run on. The layout of the
//! crate (or workspace) is decided by the design.

/// The version of this build, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_comes_from_the_manifest() {
        assert_eq!(VERSION, "0.0.0");
    }
}
