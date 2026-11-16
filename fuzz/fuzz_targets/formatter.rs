#![no_main]
use libfuzzer_sys::fuzz_target;
use wbash_core::formatter::{render, RenderOptions};
use wbash_core::handles::HandleStore;
use wbash_core::redact::Redactor;
use wbash_core::types::{Format, ToolResult};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let dir = tempfile::tempdir().unwrap();
    let redactor = Redactor::new();
    let mut handles = HandleStore::open("fuzz", dir.path(), 60, 8 << 20, Redactor::new()).unwrap();
    let mut result = ToolResult::new("fuzz", text.lines().next().unwrap_or("").to_string());
    result.must(text.lines().next().unwrap_or(""));
    for line in text.lines().skip(1) {
        result.low(line.to_string());
    }
    let _ = render(
        &result,
        RenderOptions { budget: 64, format: Format::Lean, handles: &mut handles, redactor: &redactor },
    );
});
