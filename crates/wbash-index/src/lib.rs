use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::WalkBuilder;

pub const BUILTIN_IGNORES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "vendor",
    ".next",
    "dist",
    ".venv",
    "__pycache__",
    "build",
    ".DS_Store",
    ".cache",
];

pub fn is_builtin_ignored(name: &str) -> bool {
    BUILTIN_IGNORES.contains(&name)
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub abs: PathBuf,
    pub rel: String,
    pub size: u64,
    pub mtime_ms: i64,
}

#[derive(Debug, Clone, Default)]
pub struct WalkResult {
    pub files: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DiskCache {
    v: u32,
    root: String,
    dirs: Vec<(String, i64)>,
    files: Vec<DiskFile>,
    truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DiskFile {
    rel: String,
    size: u64,
    mtime_ms: i64,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    max_entries: usize,
    cache_path: Option<PathBuf>,
    mem: Mutex<Option<std::sync::Arc<WalkResult>>>,
    content: Mutex<HashMap<String, String>>,
}

impl Workspace {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_entries: 250_000,
            cache_path: None,
            mem: Mutex::new(None),
            content: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_max_entries(mut self, n: usize) -> Self {
        self.max_entries = n;
        self
    }

    pub fn with_cache(mut self, path: PathBuf) -> Self {
        self.cache_path = Some(path);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rel(&self, abs: &Path) -> String {
        let rel = abs.strip_prefix(&self.root).unwrap_or(abs);
        let s = rel.to_string_lossy().replace('\\', "/");
        if s.is_empty() {
            ".".to_string()
        } else {
            s
        }
    }

    /// Ignore-aware file walk, sorted deterministically by relative path.
    pub fn walk(&self) -> WalkResult {
        (*self.walk_cached()).clone()
    }

    pub fn walk_cached_arc(&self) -> std::sync::Arc<WalkResult> {
        self.walk_cached()
    }

    pub fn walk_filtered(&self, filter: Option<&dyn Fn(&str) -> bool>) -> WalkResult {
        let base = self.walk_cached();
        if let Some(f) = filter {
            let files = base.files.iter().filter(|e| f(&e.rel)).cloned().collect();
            WalkResult {
                files,
                truncated: base.truncated,
            }
        } else {
            (*base).clone()
        }
    }

    fn walk_cached(&self) -> std::sync::Arc<WalkResult> {
        if let Ok(g) = self.mem.lock() {
            if let Some(w) = g.as_ref() {
                return w.clone();
            }
        }
        let w = std::sync::Arc::new(self.load_disk().unwrap_or_else(|| self.scan()));
        if let Ok(mut g) = self.mem.lock() {
            *g = Some(w.clone());
        }
        w
    }

    /// Return cached file text if available and current by mtime/size.
    pub fn cached_text(&self, rel: &str, size: u64, mtime_ms: i64) -> Option<String> {
        if let Ok(g) = self.content.lock() {
            let key = format!("{rel}\0{size}\0{mtime_ms}");
            if let Some(t) = g.get(&key) {
                return Some(t.clone());
            }
        }
        None
    }

    pub fn store_content(&self, rel: &str, size: u64, mtime_ms: i64, text: String) {
        if text.len() > 512_000 {
            return;
        }
        if let Ok(mut g) = self.content.lock() {
            if g.len() > 80_000 {
                g.clear();
            }
            g.insert(format!("{rel}\0{size}\0{mtime_ms}"), text);
        }
    }

    /// Read text preferring in-memory/disk-backed content cache.
    pub fn read_cached_text(&self, entry: &FileEntry) -> std::io::Result<TextFile> {
        if let Some(t) = self.cached_text(&entry.rel, entry.size, entry.mtime_ms) {
            return Ok(TextFile {
                content: t,
                binary: false,
                size: entry.size,
            });
        }
        let tf = read_text(&entry.abs)?;
        if !tf.binary {
            self.store_content(&entry.rel, entry.size, entry.mtime_ms, tf.content.clone());
        }
        Ok(tf)
    }

    fn scan(&self) -> WalkResult {
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .parents(true)
            .sort_by_file_name(|a, b| a.cmp(b))
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !(e.file_type().map(|t| t.is_dir()).unwrap_or(false) && is_builtin_ignored(&name))
            });

        let mut files = Vec::new();
        let mut dirs: Vec<(String, i64)> = Vec::new();
        let mut truncated = false;
        for entry in builder.build().flatten() {
            let ft = match entry.file_type() {
                Some(ft) => ft,
                None => continue,
            };
            if ft.is_dir() {
                let rel = self.rel(entry.path());
                if rel != "." && !rel.starts_with(".cache/") && rel != ".cache" {
                    if let Ok(m) = entry.metadata() {
                        dirs.push((rel, mtime_ms(&m)));
                    }
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let abs = entry.path().to_path_buf();
            let rel = self.rel(&abs);
            if rel.starts_with(".cache/") || rel == ".cache" {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let m = mtime_ms(&meta);
            files.push(FileEntry {
                abs,
                rel,
                size: meta.len(),
                mtime_ms: m,
            });
            if files.len() >= self.max_entries {
                truncated = true;
                break;
            }
        }
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        dirs.sort();
        dirs.dedup();
        let w = WalkResult { files, truncated };
        self.save_disk(&w, &dirs);
        w
    }

    fn save_disk(&self, w: &WalkResult, dirs: &[(String, i64)]) {
        let Some(path) = &self.cache_path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // metadata index (fast to parse) + compact content sidecar for warm cross-process reads/search
        let mut content_blob: Vec<u8> = Vec::new();
        let mut total = 0usize;
        let files: Vec<DiskFile> = w
            .files
            .iter()
            .map(|f| {
                let mut has = false;
                if total < 48_000_000 {
                    // read small text files for warm search cache (bounded)
                    if let Ok(tf) = read_text(&f.abs) {
                        if !tf.binary && tf.content.len() <= 64_000 {
                            let b = tf.content.as_bytes();
                            content_blob.extend_from_slice(&(b.len() as u32).to_le_bytes());
                            content_blob.extend_from_slice(b);
                            total += b.len();
                            if let Ok(mut g) = self.content.lock() {
                                g.insert(
                                    format!("{}\0{}\0{}", f.rel, f.size, f.mtime_ms),
                                    tf.content,
                                );
                            }
                            has = true;
                        }
                    }
                }
                if !has {
                    content_blob.extend_from_slice(&0u32.to_le_bytes());
                }
                DiskFile {
                    rel: f.rel.clone(),
                    size: f.size,
                    mtime_ms: f.mtime_ms,
                    text: None,
                }
            })
            .collect();
        let dc = DiskCache {
            v: 2,
            root: self.root.to_string_lossy().to_string(),
            dirs: dirs.to_vec(),
            files,
            truncated: w.truncated,
        };
        if let Ok(data) = serde_json::to_vec(&dc) {
            let _ = std::fs::write(path, data);
            let cpath = path.with_extension("content");
            let _ = std::fs::write(cpath, content_blob);
        }
    }

    fn load_disk(&self) -> Option<WalkResult> {
        let path = self.cache_path.as_ref()?;
        let data = std::fs::read(path).ok()?;
        let dc: DiskCache = serde_json::from_slice(&data).ok()?;
        if dc.v != 2 || dc.root != self.root.to_string_lossy() {
            // v1 or mismatched: drop
            let _ = std::fs::remove_file(path.with_extension("content"));
            return None;
        }
        for (rel, m) in &dc.dirs {
            if rel == "." {
                continue;
            }
            let p = self.root.join(rel);
            let meta = std::fs::metadata(&p).ok()?;
            if !meta.is_dir() || mtime_ms(&meta) != *m {
                return None;
            }
        }
        let mut files = Vec::with_capacity(dc.files.len());
        for f in &dc.files {
            files.push(FileEntry {
                abs: self.root.join(&f.rel),
                rel: f.rel.clone(),
                size: f.size,
                mtime_ms: f.mtime_ms,
            });
        }
        // load content sidecar best-effort
        let cpath = path.with_extension("content");
        if let Ok(blob) = std::fs::read(&cpath) {
            let mut off = 0usize;
            if let Ok(mut g) = self.content.lock() {
                for f in &dc.files {
                    if off + 4 > blob.len() {
                        break;
                    }
                    let len = u32::from_le_bytes(blob[off..off + 4].try_into().ok()?) as usize;
                    off += 4;
                    if len == 0 {
                        continue;
                    }
                    if off + len > blob.len() {
                        break;
                    }
                    if let Ok(s) = std::str::from_utf8(&blob[off..off + len]) {
                        g.insert(
                            format!("{}\0{}\0{}", f.rel, f.size, f.mtime_ms),
                            s.to_string(),
                        );
                    }
                    off += len;
                }
            }
        }
        Some(WalkResult {
            files,
            truncated: dc.truncated,
        })
    }

    /// Invalidate in-memory and on-disk cache (e.g. after writes).
    pub fn invalidate(&self) {
        if let Ok(mut g) = self.mem.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.content.lock() {
            g.clear();
        }
        if let Some(p) = &self.cache_path {
            let _ = std::fs::remove_file(p);
            let _ = std::fs::remove_file(p.with_extension("content"));
        }
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct TextFile {
    pub content: String,
    pub binary: bool,
    pub size: u64,
}

pub fn read_text(abs: &Path) -> std::io::Result<TextFile> {
    let bytes = std::fs::read(abs)?;
    let size = bytes.len() as u64;
    let binary = is_binary(&bytes);
    let content = String::from_utf8_lossy(&bytes).into_owned();
    Ok(TextFile {
        content,
        binary,
        size,
    })
}

pub fn is_binary(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(8192)];
    if probe.contains(&0) {
        return true;
    }
    std::str::from_utf8(probe).is_err()
}

pub fn is_test_or_vendor(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.contains("/test")
        || lower.contains("/tests/")
        || lower.starts_with("test/")
        || lower.starts_with("tests/")
        || lower.contains("_test.")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.contains("/vendor/")
        || lower.contains("/node_modules/")
        || lower.contains("/dist/")
        || lower.contains("/build/")
}

// ---------------------------------------------------------------------------
// Symbols / outlines (tree-sitter)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    Python,
    Go,
    Unknown,
}

impl Lang {
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "rs" => Lang::Rust,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" | "jsx" | "js" | "mjs" | "cjs" => Lang::Tsx,
            "py" | "pyi" => Lang::Python,
            "go" => Lang::Go,
            _ => Lang::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub indent: usize,
    pub line_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    pub heuristic: bool,
}

pub struct SymbolIndex {
    cache: Mutex<HashMap<(PathBuf, i64, u64), Vec<Symbol>>>,
}

impl Default for SymbolIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn symbols(&self, path: &Path, source: &str) -> Outline {
        let lang = Lang::from_path(path);
        if lang == Lang::Unknown {
            return Outline {
                symbols: heuristic_outline(source),
                heuristic: true,
            };
        }
        if let Ok(meta) = std::fs::metadata(path) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let key = (path.to_path_buf(), mtime, meta.len());
            if let Ok(guard) = self.cache.lock() {
                if let Some(syms) = guard.get(&key) {
                    return Outline {
                        symbols: syms.clone(),
                        heuristic: false,
                    };
                }
            }
            let syms = ts_outline(lang, source);
            if let Ok(mut guard) = self.cache.lock() {
                guard.insert(key, syms.clone());
            }
            Outline {
                symbols: syms,
                heuristic: false,
            }
        } else {
            Outline {
                symbols: ts_outline(lang, source),
                heuristic: false,
            }
        }
    }

    pub fn find_symbol(&self, path: &Path, source: &str, name: &str) -> Option<Symbol> {
        let outline = self.symbols(path, source);
        outline
            .symbols
            .into_iter()
            .filter(|s| s.name == name)
            .min_by_key(|s| (s.start_line, s.end_line))
            .or_else(|| {
                let lower = name.to_ascii_lowercase();
                let o = self.symbols(path, source);
                o.symbols
                    .into_iter()
                    .filter(|s| s.name.to_ascii_lowercase() == lower)
                    .min_by_key(|s| (s.start_line, s.end_line))
            })
    }
}

fn tree_sitter_language(lang: Lang) -> Option<tree_sitter::Language> {
    match lang {
        Lang::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Lang::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Lang::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        Lang::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Lang::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Lang::Unknown => None,
    }
}

fn is_descend_kind(kind: &str) -> bool {
    matches!(
        kind,
        "source_file"
            | "program"
            | "module"
            | "export_statement"
            | "decorated_definition"
            | "declaration_list"
            | "class_body"
            | "impl_item"
            | "trait_item"
            | "mod_item"
            | "interface_declaration"
            | "class_declaration"
            | "class_definition"
            | "abstract_class_declaration"
            | "type_declaration"
            | "internal_module"
            | "namespace_declaration"
    )
}

fn is_container_kind(kind: &str) -> bool {
    matches!(
        kind,
        "impl_item"
            | "trait_item"
            | "mod_item"
            | "interface_declaration"
            | "class_declaration"
            | "class_definition"
            | "abstract_class_declaration"
            | "internal_module"
            | "namespace_declaration"
    )
}

fn outline_kind_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "function_item" => "fn",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "trait_item" => "trait",
        "impl_item" => "impl",
        "mod_item" => "mod",
        "const_item" => "const",
        "static_item" => "static",
        "type_item" => "type",
        "macro_definition" => "macro",
        "union_item" => "union",
        "function_declaration" => "fn",
        "method_definition" => "method",
        "method_declaration" => "method",
        "class_declaration" => "class",
        "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        "lexical_declaration" => "var",
        "function_definition" => "def",
        "class_definition" => "class",
        "type_spec" => "type",
        "const_declaration" => "const",
        "var_declaration" => "var",
        _ => return None,
    })
}

fn ts_outline(lang: Lang, source: &str) -> Vec<Symbol> {
    let Some(language) = tree_sitter_language(lang) else {
        return heuristic_outline(source);
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return heuristic_outline(source);
    }
    let Some(tree) = parser.parse(source, None) else {
        return heuristic_outline(source);
    };
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    walk_node(tree.root_node(), lang, &lines, source, 0, &mut out);
    out
}

fn walk_node(
    node: tree_sitter::Node<'_>,
    lang: Lang,
    lines: &[&str],
    source: &str,
    depth: usize,
    out: &mut Vec<Symbol>,
) {
    let kind = node.kind();
    let mut child_depth = depth;

    if let Some(display_kind) = outline_kind_name(kind) {
        if kind == "lexical_declaration" && !is_function_variable(node, source) {
            // skip plain variables in outlines
        } else if let Some(name) = symbol_name(node, source, lang) {
            let start = node.start_position().row + 1;
            let text = lines
                .get(start.saturating_sub(1))
                .map(|l| l.trim().chars().take(240).collect::<String>())
                .unwrap_or_default();
            if !text.is_empty() {
                out.push(Symbol {
                    name,
                    kind: display_kind.to_string(),
                    start_line: start,
                    end_line: node.end_position().row + 1,
                    indent: depth * 2,
                    line_text: text,
                    doc: doc_before(lines, start),
                });
            }
        }
        if is_container_kind(kind) {
            child_depth = depth + 1;
        }
    }

    if !is_descend_kind(kind) && !matches!(kind, "source_file" | "program" | "module") {
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(child, lang, lines, source, child_depth, out);
    }
}

fn is_function_variable(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let text = &source[node.byte_range()];
    text.contains("=>") || text.contains("function")
}

fn symbol_name(node: tree_sitter::Node<'_>, source: &str, _lang: Lang) -> Option<String> {
    if let Some(n) = node.child_by_field_name("name") {
        return Some(source[n.byte_range()].to_string());
    }
    match node.kind() {
        "impl_item" => {
            if let Some(n) = node.child_by_field_name("type") {
                return Some(source[n.byte_range()].to_string());
            }
            let first = source[node.byte_range()].lines().next().unwrap_or("");
            Some(first.trim().trim_end_matches('{').trim().to_string())
        }
        "lexical_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    if let Some(n) = child.child_by_field_name("name") {
                        return Some(source[n.byte_range()].to_string());
                    }
                }
            }
            None
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    if let Some(n) = child.child_by_field_name("name") {
                        return Some(source[n.byte_range()].to_string());
                    }
                }
            }
            None
        }
        "export_statement" | "decorated_definition" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(name) = symbol_name(child, source, _lang) {
                    return Some(name);
                }
            }
            None
        }
        _ => None,
    }
}

fn doc_before(lines: &[&str], start_line: usize) -> Option<String> {
    if start_line < 2 {
        return None;
    }
    let mut idx = start_line - 2;
    let mut collected: Vec<String> = Vec::new();
    loop {
        let line = lines.get(idx)?.trim();
        let is_comment = line.starts_with("///")
            || line.starts_with("//!")
            || line.starts_with("//")
            || line.starts_with("#")
            || line.starts_with("/*")
            || line.starts_with('*')
            || line.starts_with("\"\"\"");
        if !is_comment {
            break;
        }
        collected.push(line.to_string());
        if idx == 0 {
            break;
        }
        idx -= 1;
    }
    if collected.is_empty() {
        return None;
    }
    collected.reverse();
    let first = &collected[0];
    let cleaned = first
        .trim_start_matches('/')
        .trim_start_matches('#')
        .trim_start_matches('*')
        .trim_start_matches("\"\"\"")
        .trim()
        .to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.chars().take(160).collect())
    }
}

/// Regex-ish heuristic outline for unsupported languages (spec §8.4 fallback).
pub fn heuristic_outline(source: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        let kind = if trimmed.starts_with("pub fn ")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("async fn ")
        {
            "fn"
        } else if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
            "def"
        } else if trimmed.starts_with("class ") {
            "class"
        } else if trimmed.starts_with("func ") {
            "fn"
        } else if trimmed.starts_with("struct ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("export interface ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("pub trait ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("pub impl ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("export type ")
        {
            "type"
        } else {
            continue;
        };
        let ident = extract_ident(trimmed);
        if ident.is_empty() {
            continue;
        }
        out.push(Symbol {
            name: ident,
            kind: kind.to_string(),
            start_line: idx + 1,
            end_line: idx + 1,
            indent: leading,
            line_text: trimmed.chars().take(240).collect(),
            doc: None,
        });
    }
    out
}

fn extract_ident(line: &str) -> String {
    let mut word = String::new();
    let mut started = false;
    for ch in line.chars() {
        let is_ident = ch.is_alphanumeric() || ch == '_' || ch == ':';
        if is_ident {
            started = true;
            word.push(ch);
        } else if started {
            break;
        }
    }
    word
}

// ---------------------------------------------------------------------------
// Fast post-edit syntax check (tree-sitter only; no compiler invocation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SyntaxError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

pub fn syntax_errors(path: &Path, source: &str) -> Vec<SyntaxError> {
    let lang = Lang::from_path(path);
    let Some(language) = tree_sitter_language(lang) else {
        return Vec::new();
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_errors(tree.root_node(), &mut out);
    out.sort_by_key(|e| (e.line, e.col));
    out
}

fn collect_errors(node: tree_sitter::Node<'_>, out: &mut Vec<SyntaxError>) {
    if node.is_error() || node.is_missing() {
        let pos = node.start_position();
        out.push(SyntaxError {
            line: pos.row + 1,
            col: pos.column + 1,
            message: if node.is_missing() {
                format!("missing {}", node.kind())
            } else {
                format!("unexpected {}", node.kind())
            },
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.has_error() || child.is_missing() {
            collect_errors(child, out);
        }
    }
}
