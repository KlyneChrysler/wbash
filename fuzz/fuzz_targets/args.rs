#![no_main]
use libfuzzer_sys::fuzz_target;
use wbash_core::args::parse_line_range;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    for len in [0usize, 1, 10, 1000] {
        let _ = parse_line_range(&s, len);
    }
});
