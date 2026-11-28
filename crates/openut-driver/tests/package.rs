use serde_json::{Value, from_str};
use std::path::Path;

#[test]
fn steamvr_driver_name_matches_the_package_and_library() {
    let manifest: Value = from_str(include_str!("../package/openut/driver.vrdrivermanifest"))
        .expect("valid SteamVR driver manifest");
    assert_eq!(manifest["name"], "openut");
    assert_eq!(manifest["alwaysActivate"], true);
    assert_eq!(manifest["resourceOnly"], false);
    assert!(include_str!("../Cargo.toml").contains("name = \"driver_openut\""));
}

#[test]
fn ships_the_local_bridge_setting_and_pose_only_profile() {
    let settings: Value = from_str(include_str!(
        "../package/openut/resources/settings/default.vrsettings"
    ))
    .expect("valid SteamVR driver settings");
    assert_eq!(settings["driver_openut"]["bridge_port"], 27_316);
    let profile: Value = from_str(include_str!(
        "../package/openut/resources/input/openut_tracker_profile.json"
    ))
    .expect("valid SteamVR input profile");
    assert_eq!(profile["device_class"], "TrackedDeviceClass_GenericTracker");
    assert_eq!(profile["input_source"], serde_json::json!({}));
}

#[test]
fn ffi_is_confined_to_the_abi_module() {
    let mut folders = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(folder) = folders.pop() {
        for entry in folder.read_dir().expect("read driver source directory") {
            let path = entry.expect("read driver source entry").path();
            if path.file_name().is_some_and(|name| name == "abi") {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let rust = std::fs::read_to_string(&path).expect("read Rust source");
                for line in rust.lines().filter(|line| line.contains("unsafe")) {
                    assert!(
                        path.ends_with("src/lib.rs") && line == "#[allow(unsafe_code)]",
                        "unexpected FFI declaration in {}",
                        path.display()
                    );
                }
            }
        }
    }
}
