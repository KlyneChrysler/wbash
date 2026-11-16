use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use wbash_core::error::{Result, WErr};
use wbash_core::handles::HandleStore;
use wbash_core::types::{Diagnostic, HandleRef, Item, ItemClass, Severity, TestResult, ToolResult};

pub fn display_path(root: &Path, abs: &Path) -> String {
    if abs == root {
        ".".to_string()
    } else {
        wbash_index::Workspace::new(root).rel(abs)
    }
}

pub fn rel_path(root: &Path, p: &str) -> String {
    let path = Path::new(p);
    if path.is_absolute() {
        wbash_index::Workspace::new(root).rel(path)
    } else {
        p.replace('\\', "/")
    }
}

pub fn globset(glob: Option<&str>) -> Result<Option<GlobSet>> {
    let Some(pattern) = glob else {
        return Ok(None);
    };
    let glob = Glob::new(pattern).map_err(|e| {
        WErr::invalid_args(
            format!("bad glob '{pattern}': {e}"),
            "use a glob like 'src/**/*.rs'",
        )
    })?;
    let mut b = GlobSetBuilder::new();
    b.add(glob);
    b.build().map(Some).map_err(|e| {
        WErr::invalid_args(
            format!("bad glob '{pattern}': {e}"),
            "use a glob like 'src/**/*.rs'",
        )
    })
}

pub fn matches_glob(gs: Option<&GlobSet>, rel: &str) -> bool {
    match gs {
        Some(g) => g.is_match(rel),
        None => true,
    }
}

pub fn under(path: &str, base: &str) -> bool {
    base == "."
        || path == base
        || path
            .strip_prefix(base)
            .map(|r| r.starts_with('/'))
            .unwrap_or(false)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| WErr::invalid_args("path has no parent directory", "use a file path"))?;
    let tmp = parent.join(format!(
        ".{}.w-bash-{}.tmp",
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string()),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| {
            WErr::new(
                "permission_denied",
                format!("create {}: {e}", tmp.display()),
            )
        })?;
        f.write_all(bytes)
            .map_err(|e| WErr::new("permission_denied", format!("write {}: {e}", tmp.display())))?;
        f.sync_all()
            .map_err(|e| WErr::new("internal", format!("fsync {}: {e}", tmp.display())))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        WErr::new(
            "permission_denied",
            format!("rename {}: {e}", path.display()),
        )
    })?;
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c == '\r' {
            // Progress rewrites: keep only the text after the last CR.
            let mut tail = String::new();
            for c in chars.by_ref() {
                if c == '\n' {
                    tail.push(c);
                    break;
                }
                tail.push(c);
            }
            out.push_str(&tail);
        } else {
            out.push(c);
        }
    }
    out
}

pub fn normalize_diagnostics(root: &Path, diags: &mut Vec<Diagnostic>) {
    for d in diags.iter_mut() {
        d.file = rel_path(root, &d.file);
    }
    diags.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.code.cmp(&b.code))
            .then(a.message.cmp(&b.message))
    });
    diags.dedup_by(|a, b| {
        a.file == b.file
            && a.line == b.line
            && a.col == b.col
            && a.code == b.code
            && a.message == b.message
    });
}

pub fn diagnostics_into(
    result: &mut ToolResult,
    mut diags: Vec<Diagnostic>,
    root: &Path,
    handles: &mut HandleStore,
) -> Result<()> {
    normalize_diagnostics(root, &mut diags);
    if diags.is_empty() {
        return Ok(());
    }
    result.diagnostics.extend(diags.clone());

    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, d) in diags.iter().enumerate() {
        let key = (d.code.clone().unwrap_or_default(), d.message.clone());
        groups.entry(key).or_default().push(i);
    }

    let mut flooded: BTreeSet<usize> = BTreeSet::new();
    for idxs in groups.values() {
        if idxs.len() > 20 {
            let occurrences: Vec<String> = idxs.iter().map(|&i| diags[i].render()).collect();
            let href = handles.put("flood", occurrences.join("\n").as_bytes(), "text")?;
            let first = diags[idxs[0]].clone();
            let mut msg = format!("{} ({} occurrences)", first.message, idxs.len());
            if let Some(code) = &first.code {
                msg = format!("{code} {msg}");
            }
            result.push(Item::new(
                ItemClass::MustKeep,
                1000.0,
                vec![
                    format!("{} {}", first.severity.as_str().to_uppercase(), msg),
                    format!("...+{} lines ({})", idxs.len(), href.id),
                ],
            ));
            result.handle(HandleRef {
                id: href.id.clone(),
                kind: href.kind.clone(),
                bytes: href.bytes,
                hash: href.hash.clone(),
            });
            for &i in idxs {
                flooded.insert(i);
            }
        }
    }

    for (i, d) in diags.iter().enumerate() {
        if flooded.contains(&i) {
            continue;
        }
        let class = if d.severity == Severity::Error {
            ItemClass::MustKeep
        } else {
            ItemClass::Low
        };
        result.push(Item::new(
            class,
            if class == ItemClass::MustKeep {
                1000.0
            } else {
                5.0
            },
            vec![d.render()],
        ));
    }
    Ok(())
}

/// Keep all failure records (I1), plus a compact pass summary.
pub fn tests_into(result: &mut ToolResult, tests: &[TestResult], paths_relative: bool) {
    for t in tests.iter().filter(|t| t.status.is_failure()) {
        let mut line = format!("{} {}", t.status.as_str().to_uppercase(), t.id);
        if let Some(msg) = &t.message {
            let msg = msg.lines().next().unwrap_or("").trim();
            if !msg.is_empty() {
                line.push_str(": ");
                line.push_str(&msg.chars().take(300).collect::<String>());
            }
        }
        if let Some(loc) = &t.location {
            let loc = if paths_relative {
                loc.replace('\\', "/")
            } else {
                loc.clone()
            };
            if !loc.is_empty() && !t.id.contains(&loc) {
                line.push_str(&format!(" ({loc})"));
            }
        }
        result.must(line);
    }
}

pub fn counts_of(tests: &[TestResult]) -> (usize, usize, usize) {
    let passed = tests
        .iter()
        .filter(|t| matches!(t.status, wbash_core::types::TestStatus::Pass))
        .count();
    let failed = tests.iter().filter(|t| t.status.is_failure()).count();
    let skipped = tests.len().saturating_sub(passed + failed);
    (passed, failed, skipped)
}

pub fn no_ansi_lines(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn is_toolchain_frame(line: &str) -> bool {
    let l = line.trim();
    (l.starts_with("at ")
        || l.contains("::")
        || l.contains(".rs:")
        || l.contains(".go:")
        || l.contains("node_modules")
        || l.contains("site-packages"))
        && (l.contains("/rustc/")
            || l.contains("/rustlib/")
            || l.contains("library/std")
            || l.contains("library/core")
            || l.contains("library/alloc")
            || l.contains("site-packages/_pytest")
            || l.contains("node_modules/")
            || l.contains("internal/modules")
            || l.contains("<frozen "))
}

pub fn strip_toolchain_frames(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut dropped = 0usize;
    for line in lines {
        if is_toolchain_frame(&line) {
            dropped += 1;
            continue;
        }
        if dropped > 0 {
            out.push(format!("...+{dropped} std frames"));
            dropped = 0;
        }
        out.push(line);
    }
    if dropped > 0 {
        out.push(format!("...+{dropped} std frames"));
    }
    out
}

pub fn is_error_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("error")
        || l.contains("failed")
        || l.contains("failure")
        || l.contains("exception")
        || l.contains("panic")
        || l.contains("traceback")
        || l.contains("segfault")
        || l.contains("fatal")
        || l.contains("undefined reference")
        || line.contains("✗")
        || line.contains("error[")
}

fn is_error_context_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("-->")
        || t.starts_with('|')
        || t.starts_with("= help:")
        || t.starts_with("= note:")
        || t.starts_with("help:")
        || t.starts_with("note:")
        || (t.starts_with(' ') && t.contains('^'))
}

/// Generic compaction fallback (spec §7.5): head + error windows + tail.
pub fn compact_tail(
    result: &mut ToolResult,
    raw: &str,
    label: &str,
    handles: &mut HandleStore,
    force_full: bool,
) -> Result<()> {
    let mut lines = no_ansi_lines(raw);
    if lines.is_empty() {
        return Ok(());
    }
    lines = strip_toolchain_frames(lines);
    let raw_bytes = raw.len() as u64;
    let need_full = force_full || raw_bytes > 4096;
    let h = handles.put(label, raw.as_bytes(), "text")?;
    result.handle(HandleRef {
        id: h.id.clone(),
        kind: h.kind.clone(),
        bytes: h.bytes,
        hash: h.hash.clone(),
    });
    let href = h.id.clone();

    let head_n = 8.min(lines.len());
    let tail_n = 6.min(lines.len().saturating_sub(head_n));
    let mut selected: Vec<(usize, &str)> = Vec::new();
    for (i, l) in lines.iter().enumerate().take(head_n) {
        selected.push((i, l.as_str()));
    }
    for (i, l) in lines.iter().enumerate() {
        if i < head_n || i >= lines.len().saturating_sub(tail_n) {
            continue;
        }
        if is_error_line(l) {
            let end = (i + 5).min(lines.len());
            for (j, lj) in lines.iter().enumerate().take(end).skip(i) {
                selected.push((j, lj.as_str()));
                // keep contiguous context/help block a bit longer if relevant
                if j > i
                    && !is_error_line(lj.as_str())
                    && !is_error_context_line(lj.as_str())
                    && lj.trim().is_empty()
                {
                    break;
                }
            }
        } else if is_error_context_line(l) {
            // include context lines that may be adjacent to errors outside head/tail scan
            selected.push((i, l.as_str()));
        }
    }
    for (i, l) in lines
        .iter()
        .enumerate()
        .skip(lines.len().saturating_sub(tail_n))
    {
        selected.push((i, l.as_str()));
    }
    selected.sort_by_key(|(i, _)| *i);
    selected.dedup_by_key(|(i, _)| *i);

    let mut omitted = 0usize;
    let mut last_emitted: Option<usize> = None;
    let mut stream: Vec<(usize, &str, ItemClass)> = Vec::with_capacity(selected.len());
    for (i, line) in selected {
        let class = if is_error_line(line) || is_error_context_line(line) {
            ItemClass::MustKeep
        } else {
            ItemClass::Low
        };
        stream.push((i, line, class));
    }
    let mut j = 0usize;
    while j < stream.len() {
        let (i, line, class) = stream[j];
        if let Some(prev) = last_emitted {
            if i > prev + 1 {
                result.low(format!("...+{} lines (~{href})", i - prev - 1));
                omitted += i - prev - 1;
            }
        }
        if class == ItemClass::Low {
            let mut k = j + 1;
            while k < stream.len()
                && stream[k].2 == ItemClass::Low
                && stream[k].1 == line
                && stream[k].0 == i + (k - j)
            {
                k += 1;
            }
            let count = k - j;
            if count > 1 {
                result.low(line.to_string());
                result.low(format!("  ×{count} repeated"));
            } else {
                result.push(Item::new(ItemClass::Low, 2.0, vec![line.to_string()]));
            }
            last_emitted = Some(stream[k - 1].0);
            j = k;
            continue;
        }
        result.push(Item::new(
            ItemClass::MustKeep,
            1000.0,
            vec![line.to_string()],
        ));
        last_emitted = Some(i);
        j += 1;
    }
    if let Some(last) = last_emitted {
        if last + 1 < lines.len() {
            omitted += lines.len() - last - 1;
        }
    }
    let full_line = format!("full: {href}");
    if need_full {
        if !result
            .items
            .iter()
            .any(|i| i.lines.iter().any(|l| l == &full_line))
        {
            result.must(full_line);
        }
    } else if omitted > 0 {
        result.must(format!("...+{omitted} lines ({href})"));
    }
    Ok(())
}

pub fn path_buf(root: &Path, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        root.join(rel)
    }
}
