#![no_main]
use std::path::Path;
use libfuzzer_sys::fuzz_target;
use wbash_core::policy;

fuzz_target!(|data: &[u8]| {
    let raw = String::from_utf8_lossy(data);
    let _ = policy::normalize(Path::new(raw.as_ref()));
    if let Ok(root) = std::env::temp_dir().canonicalize() {
        let _ = policy::resolve(&root, &raw, &[], false);
    }
});
