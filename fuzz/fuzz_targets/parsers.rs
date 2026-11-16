#![no_main]
use libfuzzer_sys::fuzz_target;
use wbash_core::parsers::ParserSet;
use wbash_core::types::Exit;
use wbash_parsers::ParserRegistry;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let reg = ParserRegistry::new();
    for argv in [
        vec!["cargo".to_string(), "test".to_string()],
        vec!["pytest".to_string()],
        vec!["tsc".to_string()],
        vec!["go".to_string(), "test".to_string()],
        vec!["ruff".to_string(), "check".to_string()],
    ] {
        let _ = reg.parse(&argv, &Exit::Code(1), &text, &text);
    }
});
