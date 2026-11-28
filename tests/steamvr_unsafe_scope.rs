use std::fs;
use std::path::Path;

#[test]
fn application_unsafe_code_is_confined_to_the_steamvr_ffi_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    check_source(&src, &src.join("steamvr/ffi.rs"));
}

fn check_source(path: &Path, ffi: &Path) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read source directory") {
            check_source(&entry.expect("read source entry").path(), ffi);
        }
    } else if path != ffi && path.extension().is_some_and(|ext| ext == "rs") {
        let source = fs::read_to_string(path).expect("read Rust source");
        assert!(
            !source
                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                .any(|word| matches!(word, "unsafe" | "unsafe_code")),
            "only steamvr/ffi.rs may contain unsafe Rust: {}",
            path.display()
        );
    }
}
