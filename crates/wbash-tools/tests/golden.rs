use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use wbash_core::config::Config;
use wbash_core::dispatcher::Dispatcher;
use wbash_parsers::ParserRegistry;

fn copy_fixture(dst: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/repos/basic");
    for rel in [
        "Cargo.toml",
        "src/config.rs",
        "src/main.rs",
        "tests/basic.rs",
    ] {
        let to = dst.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(src.join(rel), to).unwrap();
    }
}

fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn golden_wire_outputs() {
    let root: PathBuf = std::env::temp_dir().join(format!("wbash-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    std::env::set_var("XDG_CACHE_HOME", root.with_extension("cache"));
    std::env::set_var("XDG_CONFIG_HOME", root.with_extension("config"));
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wbash_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();

    let tree = d.call("tree", &json!({}), "g");
    assert_eq!(tree.text, golden("tree/basic.lean"));

    let search = d.call("search", &json!({"pattern": "parse_config"}), "g");
    assert_eq!(search.text, golden("search/basic.lean"));

    let missing = d.call("read", &json!({"path": "nope.rs"}), "g");
    assert_eq!(missing.text, golden("errors/not_found.lean"));
}
