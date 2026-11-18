use std::sync::Arc;

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};

use wbash_core::args::parse_args;
use wbash_core::error::{Result, WErr};
use wbash_core::registry::{Tool, ToolContext};
use wbash_core::types::{Tier, ToolResult};

use crate::util;

pub struct ReplaceTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceArgs {
    find: String,
    replace: String,
    #[serde(default = "dot")]
    path: String,
    #[serde(default)]
    paths: Vec<String>,
    glob: Option<String>,
    #[serde(default)]
    word: bool,
    #[serde(default)]
    regex: bool,
    #[serde(default)]
    dry_run: bool,
    #[serde(default = "max_files_default")]
    max_files: u32,
    limit: Option<u32>,
}

fn dot() -> String {
    ".".to_string()
}
fn max_files_default() -> u32 {
    200
}

enum Matcher {
    Literal(String),
    Word(Regex),
    Regex(Regex),
}

impl Matcher {
    fn count(&self, text: &str) -> usize {
        match self {
            Matcher::Literal(f) => text.match_indices(f).count(),
            Matcher::Word(re) | Matcher::Regex(re) => re.find_iter(text).count(),
        }
    }
    fn replace_all(&self, text: &str, repl: &str, limit: Option<usize>) -> (String, usize) {
        match self {
            Matcher::Literal(f) => replace_literal(text, f, repl, limit),
            Matcher::Word(re) | Matcher::Regex(re) => {
                if let Some(lim) = limit {
                    let mut n = 0usize;
                    let out = re
                        .replace_all(text, |caps: &regex::Captures<'_>| {
                            if n < lim {
                                n += 1;
                                expand_replacement(repl, caps)
                            } else {
                                caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
                            }
                        })
                        .into_owned();
                    (out, n)
                } else {
                    let mut n = 0usize;
                    for _ in re.find_iter(text) {
                        n += 1;
                    }
                    (re.replace_all(text, repl).into_owned(), n)
                }
            }
        }
    }
}

fn replace_literal(text: &str, find: &str, repl: &str, limit: Option<usize>) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut n = 0usize;
    for (i, _) in text.match_indices(find) {
        if limit.map(|l| n >= l).unwrap_or(false) {
            break;
        }
        out.push_str(&text[last..i]);
        out.push_str(repl);
        last = i + find.len();
        n += 1;
    }
    out.push_str(&text[last..]);
    (out, n)
}

fn expand_replacement(template: &str, caps: &regex::Captures<'_>) -> String {
    // simple $0/$1.. expansion; good enough for agent use
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'$') {
                chars.next();
                out.push('$');
                continue;
            }
            if chars.peek() == Some(&'0') {
                chars.next();
                if let Some(m) = caps.get(0) {
                    out.push_str(m.as_str());
                }
                continue;
            }
            let mut num = String::new();
            while let Some(d) = chars.peek().copied() {
                if d.is_ascii_digit() {
                    num.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if !num.is_empty() {
                if let Ok(i) = num.parse::<usize>() {
                    if let Some(m) = caps.get(i) {
                        out.push_str(m.as_str());
                    }
                } else {
                    out.push('$');
                    out.push_str(&num);
                }
                continue;
            }
            out.push('$');
        } else {
            out.push(c);
        }
    }
    out
}

impl Tool for ReplaceTool {
    fn name(&self) -> &'static str {
        "replace"
    }
    fn description(&self) -> &'static str {
        "Replace matches across files."
    }
    fn min_tier(&self) -> Tier {
        Tier::WriteWorkspace
    }
    fn destructive(&self) -> bool {
        true
    }
    fn idempotent(&self) -> bool {
        false
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "find": {"type": "string"},
                "replace": {"type": "string"},
                "path": {"type": "string"},
                "paths": {"type": "array", "items": {"type": "string"}},
                "glob": {"type": "string"},
                "word": {"type": "boolean"},
                "regex": {"type": "boolean"},
                "dry_run": {"type": "boolean"},
                "max_files": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1}
            },
            "required": ["find", "replace"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: ReplaceArgs = parse_args(
            args.clone(),
            "replace",
            "pass find and replace plus optional path/paths/glob/word/regex/dry_run/max_files/limit",
        )?;
        if a.find.is_empty() {
            return Err(WErr::invalid_args(
                "find must not be empty",
                "pass the text or pattern to replace",
            ));
        }
        if a.word && a.regex {
            return Err(WErr::invalid_args(
                "word and regex cannot both be true",
                "use word for literal word matches or regex for patterns",
            ));
        }
        let matcher = if a.regex {
            Matcher::Regex(Regex::new(&a.find).map_err(|e| {
                WErr::invalid_args(
                    format!("invalid regex '{}': {e}", a.find),
                    "check escaping or use literal mode",
                )
            })?)
        } else if a.word {
            let pat = format!(r"\b{}\b", regex::escape(&a.find));
            Matcher::Word(Regex::new(&pat).map_err(|e| {
                WErr::invalid_args(
                    format!("invalid word pattern: {e}"),
                    "check the find string",
                )
            })?)
        } else {
            Matcher::Literal(a.find.clone())
        };

        let mut bases_rel = Vec::new();
        if a.paths.is_empty() {
            let base = ctx.resolve(&a.path, true)?;
            bases_rel.push(ctx.rel(&base));
        } else {
            for p in &a.paths {
                let base = ctx.resolve(p, true)?;
                bases_rel.push(ctx.rel(&base));
            }
        }
        let gs = util::globset(a.glob.as_deref())?;
        let walk = ctx.workspace.walk_cached_arc();

        let mut candidates: Vec<&wbash_index::FileEntry> = Vec::new();
        for f in walk.files.iter() {
            let in_scope = bases_rel.iter().any(|b| util::under(&f.rel, b))
                && util::matches_glob(gs.as_ref(), &f.rel);
            if in_scope {
                candidates.push(f);
            }
        }
        if candidates.len() > a.max_files as usize {
            return Err(WErr::with_hint(
                "too_many_files",
                format!("{} files in scope (max {})", candidates.len(), a.max_files),
                "narrow path/glob or raise max_files",
            ));
        }

        let mut total_replacements = 0usize;
        let mut files_changed = 0usize;
        let mut diff_lines: Vec<String> = Vec::new();
        let mut remaining_limit = a.limit.map(|l| l as usize);
        const MAX_DIFF_LINES: usize = 2000;

        for f in candidates {
            if remaining_limit == Some(0) {
                break;
            }
            let Ok(tf) = wbash_index::read_text(&f.abs) else {
                continue;
            };
            if tf.binary {
                continue;
            }
            let old = tf.content;
            let n = matcher.count(&old);
            if n == 0 {
                continue;
            }
            let apply_n = remaining_limit.map(|l| l.min(n)).unwrap_or(n);
            let (new, replaced) = matcher.replace_all(&old, &a.replace, Some(apply_n));
            if replaced == 0 || new == old {
                continue;
            }
            total_replacements += replaced;
            files_changed += 1;
            if let Some(ref mut lim) = remaining_limit {
                *lim = lim.saturating_sub(replaced);
            }

            if diff_lines.len() < MAX_DIFF_LINES {
                let file_diffs = render_file_diff(&f.rel, &old, &new);
                for l in file_diffs {
                    if diff_lines.len() >= MAX_DIFF_LINES {
                        break;
                    }
                    diff_lines.push(l);
                }
            }

            if !a.dry_run {
                let bytes = match tf.encoding.unwrap_or(wbash_index::TextEncoding::Utf8) {
                    enc @ (wbash_index::TextEncoding::Utf16Le
                    | wbash_index::TextEncoding::Utf16Be
                    | wbash_index::TextEncoding::Latin1) => wbash_index::encode_text(&new, enc)
                        .map_err(|e| {
                            WErr::new("invalid_args", format!("cannot encode replace: {e}"))
                        })?,
                    wbash_index::TextEncoding::Utf8 => new.as_bytes().to_vec(),
                };
                util::atomic_write(&f.abs, &bytes)?;
                ctx.ledger.invalidate_path(&f.rel);
                if let Ok(meta) = std::fs::metadata(&f.abs) {
                    let m = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    ctx.workspace.invalidate_file(&f.rel, meta.len(), m);
                }
            }
        }

        if total_replacements == 0 {
            return Err(WErr::with_hint(
                "replace_not_found",
                "0 matches",
                "check find/glob/path; use word=false or regex for patterns",
            ));
        }

        let header = if a.dry_run {
            format!(
                "dry_run: {} in {}",
                util::plural(total_replacements, "match"),
                util::plural(files_changed, "file")
            )
        } else {
            format!(
                "replaced {} in {}",
                util::plural(total_replacements, "match"),
                util::plural(files_changed, "file")
            )
        };
        let mut result = ToolResult::new("replace", header);
        for l in diff_lines {
            result.low(l);
        }
        result
            .meta
            .counts
            .insert("replacements".into(), total_replacements as i64);
        result
            .meta
            .counts
            .insert("files".into(), files_changed as i64);
        if a.dry_run {
            result.note("dry_run");
        }
        Ok(result)
    }
}

fn render_file_diff(path: &str, old: &str, new: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                old_line += 1;
                out.push(format!(
                    "{}:{}: -{}",
                    path,
                    old_line,
                    change.value().trim_end_matches(['\n', '\r'])
                ));
            }
            ChangeTag::Insert => {
                new_line += 1;
                out.push(format!(
                    "{}:{}: +{}",
                    path,
                    new_line,
                    change.value().trim_end_matches(['\n', '\r'])
                ));
            }
        }
    }
    out
}

pub fn register(out: &mut Vec<Arc<dyn Tool>>) {
    out.push(Arc::new(ReplaceTool));
}
