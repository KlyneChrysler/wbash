use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
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

#[test]
fn search_and_tree_match_reference_tools() {
    let root: PathBuf = std::env::temp_dir().join(format!("wbash-diff-{}", std::process::id()));
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

    let ours = d.call("search", &json!({"pattern": "parse_config"}), "d");
    let ours: BTreeSet<(String, u32)> = ours
        .text
        .lines()
        .filter_map(|l| {
            let (path, rest) = l.split_once(':')?;
            let line = rest.split(':').next()?.parse().ok()?;
            Some((path.to_string(), line))
        })
        .collect();

    let grep = Command::new("grep")
        .args(["-rna", "--exclude-dir=.git", "parse_config", "."])
        .current_dir(&root)
        .output()
        .expect("grep available");
    let theirs: BTreeSet<(String, u32)> = String::from_utf8_lossy(&grep.stdout)
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("./")?;
            let (path, rest) = rest.split_once(':')?;
            let line = rest.split(':').next()?.parse().ok()?;
            Some((path.to_string(), line))
        })
        .collect();
    assert_eq!(ours, theirs);

    let tree = d.call("tree", &json!({}), "d");
    let mut ours: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = Vec::new();
    for line in tree.text.lines().skip(1) {
        if line.trim().is_empty() || line.contains("...+") {
            continue;
        }
        let depth = (line.len() - line.trim_start().len()) / 2;
        stack.truncate(depth);
        let name = line.trim().split(" (").next().unwrap();
        if name.ends_with('/') {
            stack.push(name.trim_end_matches('/').to_string());
        } else {
            let mut path = stack.clone();
            path.push(name.to_string());
            ours.insert(path.join("/"));
        }
    }
    let find = Command::new("find")
        .args([".", "-type", "f", "-not", "-path", "./.git/*"])
        .current_dir(&root)
        .output()
        .expect("find available");
    let theirs: BTreeSet<String> = String::from_utf8_lossy(&find.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("./").map(|s| s.trim().to_string()))
        .collect();
    assert_eq!(ours, theirs, "tree files");
}
