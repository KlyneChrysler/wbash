use serde::Deserialize;
use serde_json::{json, Value};

use wbash_core::args::parse_args;
use wbash_core::error::{Result, WErr};
use wbash_core::registry::{Tool, ToolContext};
use wbash_core::types::{HandleRef, Item, ItemClass, Tier, ToolResult};

use crate::util;

pub struct SearchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    pattern: String,
    #[serde(default = "text")]
    mode: String,
    #[serde(default = "dot")]
    path: String,
    glob: Option<String>,
    #[serde(default)]
    context: u32,
    limit: Option<u32>,
}

fn text() -> String {
    "text".to_string()
}
fn dot() -> String {
    ".".to_string()
}

#[derive(Debug, Clone)]
struct Hit {
    path: String,
    line: u32,
    col: u32,
    snippet: String,
    is_definition: bool,
    score: i32,
}

impl Tool for SearchTool {
    fn name(&self) -> &'static str {
        "search"
    }
    fn description(&self) -> &'static str {
        "Search text, regex or symbols across the workspace."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "mode": {"type": "string", "enum": ["text", "regex", "symbol"], "default": "text"},
                "path": {"type": "string", "default": "."},
                "glob": {"type": "string"},
                "context": {"type": "integer", "minimum": 0, "maximum": 20, "default": 0},
                "limit": {"type": "integer", "minimum": 1}
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: SearchArgs = parse_args(
            args.clone(),
            "search",
            "pass pattern plus optional mode, path, glob, context, limit",
        )?;
        if a.pattern.is_empty() {
            return Err(WErr::invalid_args(
                "pattern must not be empty",
                "pattern=<text>",
            ));
        }
        if a.context > ctx.config.search.context_max {
            return Err(WErr::invalid_args(
                format!(
                    "context {} exceeds max {}",
                    a.context, ctx.config.search.context_max
                ),
                format!("context must be 0..={}", ctx.config.search.context_max),
            ));
        }
        let limit = a.limit.unwrap_or(ctx.config.search.default_limit);
        if limit == 0 || limit > ctx.config.search.max_limit {
            return Err(WErr::invalid_args(
                format!("limit {limit} out of range"),
                format!("limit must be 1..={}", ctx.config.search.max_limit),
            ));
        }
        let base = ctx.resolve(&a.path, true)?;
        let base_rel = ctx.rel(&base);
        let gs = util::globset(a.glob.as_deref())?;
        let regex = if a.mode == "regex" {
            Some(regex::Regex::new(&a.pattern).map_err(|e| {
                WErr::invalid_args(
                    format!("invalid regex '{}': {e}", a.pattern),
                    "check escaping or use mode=text",
                )
            })?)
        } else {
            None
        };

        let mut hits: Vec<Hit> = Vec::new();
        let mut files_with_hits = 0usize;
        let mut fallback = false;
        let mut scanned_files = 0usize;

        let walk = ctx.workspace.walk_cached_arc();
        let filter =
            |rel: &str| util::under(rel, &base_rel) && util::matches_glob(gs.as_ref(), rel);
        for f in walk.files.iter() {
            if !filter(&f.rel) {
                continue;
            }
            scanned_files += 1;
            let Ok(text) = ctx.workspace.read_cached_text(f) else {
                continue;
            };
            if text.binary {
                continue;
            }
            if a.mode == "symbol" {
                let outline = ctx.symbols.symbols(&f.abs, &text.content);
                if outline.heuristic {
                    fallback = true;
                }
                let before = hits.len();
                for s in outline.symbols {
                    let name_lc = s.name.to_ascii_lowercase();
                    let pat_lc = a.pattern.to_ascii_lowercase();
                    if s.name == a.pattern || name_lc.contains(&pat_lc) {
                        let mut score = 10;
                        if s.name == a.pattern {
                            score += 8;
                        } else if name_lc == pat_lc {
                            score += 6;
                        }
                        score += path_boost(&f.rel, &pat_lc);
                        hits.push(Hit {
                            path: f.rel.clone(),
                            line: s.start_line as u32,
                            col: 1,
                            snippet: format!("{} {}", s.kind, s.line_text),
                            is_definition: true,
                            score,
                        });
                    }
                }
                if hits.len() > before {
                    files_with_hits += 1;
                }
                continue;
            }
            let before = hits.len();
            for (idx, line) in text.content.lines().enumerate() {
                let matched = match &regex {
                    Some(re) => re.is_match(line),
                    None => line.contains(&a.pattern),
                };
                if matched {
                    let col = match &regex {
                        Some(re) => re.find(line).map(|m| m.start() + 1).unwrap_or(1),
                        None => line.find(&a.pattern).map(|i| i + 1).unwrap_or(1),
                    };
                    let snippet = line.trim().chars().take(240).collect::<String>();
                    let is_definition = looks_like_definition(line);
                    let pat_lc = a.pattern.to_ascii_lowercase();
                    let mut score = if is_definition { 8 } else { 3 };
                    if a.mode != "regex" {
                        let line_lc = line.to_ascii_lowercase();
                        if line_lc.contains(&format!(" {pat_lc} "))
                            || line_lc.starts_with(&pat_lc)
                            || line_lc.ends_with(&pat_lc)
                        {
                            score += 2;
                        }
                    }
                    score += path_boost(&f.rel, &pat_lc);
                    hits.push(Hit {
                        path: f.rel.clone(),
                        line: (idx + 1) as u32,
                        col: col as u32,
                        snippet,
                        is_definition,
                        score,
                    });
                }
            }
            if hits.len() > before {
                files_with_hits += 1;
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(b.is_definition.cmp(&a.is_definition))
                .then(a.path.cmp(&b.path))
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });

        let total = hits.len();
        let truncated = total > limit as usize;
        let returned = hits.len().min(limit as usize);
        let omitted_lines: Vec<String> = hits
            .iter()
            .skip(returned)
            .map(|h| format!("{}:{}: {}", h.path, h.line, h.snippet))
            .collect();
        hits.truncate(returned);

        let mut result = ToolResult::new(
            "search",
            if truncated {
                format!("{files_with_hits} files, {total} matches ({returned} returned)")
            } else {
                format!("{files_with_hits} files, {total} matches")
            },
        );
        if a.mode == "symbol" && fallback {
            result.note("symbol_index_unavailable fallback_scan");
        }
        if walk.truncated {
            result.note("search_truncated");
        }

        let mut context_lines: Vec<String> = Vec::new();
        // Group consecutive hits in same file with identical snippet as path:l1,l2
        let mut i = 0usize;
        while i < hits.len() {
            let hit = &hits[i];
            let class = if hit.is_definition {
                ItemClass::High
            } else {
                ItemClass::Low
            };
            let mut lines = vec![hit.line];
            let mut j = i + 1;
            while j < hits.len()
                && hits[j].path == hit.path
                && hits[j].snippet == hit.snippet
                && hits[j].is_definition == hit.is_definition
            {
                if let Some(last) = lines.last().copied() {
                    if hits[j].line <= last + 1 + a.context { /* keep separate for context clarity */
                    }
                }
                // fold only identical adjacent snippet records with no gaps requested
                if a.context == 0 {
                    lines.push(hits[j].line);
                    j += 1;
                } else {
                    break;
                }
            }
            let loc = if lines.len() == 1 {
                format!("{}:{}", hit.path, lines[0])
            } else {
                let joined = lines
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}:{}", hit.path, joined)
            };
            result.push(Item::new(
                class,
                if hit.is_definition { 8.0 } else { 3.0 },
                vec![format!("{}: {}", loc, hit.snippet)],
            ));
            if a.context > 0 {
                if let Ok(t) = wbash_index::read_text(&ctx.root.join(&hit.path)) {
                    let all: Vec<&str> = t.content.lines().collect();
                    let lo = (hit.line as usize).saturating_sub(a.context as usize + 1);
                    let hi = ((hit.line as usize) + a.context as usize).min(all.len());
                    for (k, l) in all.iter().enumerate().take(hi).skip(lo) {
                        let n = k + 1;
                        if n as u32 == hit.line {
                            continue;
                        }
                        context_lines.push(format!("{}:{}- {}", hit.path, n, l.trim()));
                    }
                }
            }
            i = j;
        }
        for line in context_lines {
            result.low(line);
        }

        if truncated {
            let omitted = total - returned;
            let body = omitted_lines.join("\n");
            let h = ctx.handles.put("search", body.as_bytes(), "text")?;
            result.must(format!("...+{omitted} more ({})", h.id));
            result.handle(HandleRef {
                id: h.id.clone(),
                kind: h.kind.clone(),
                bytes: h.bytes,
                hash: h.hash.clone(),
            });
        }

        result
            .meta
            .counts
            .insert("files".into(), files_with_hits as i64);
        result.meta.counts.insert("matches".into(), total as i64);
        result
            .meta
            .counts
            .insert("returned".into(), returned as i64);
        result
            .meta
            .counts
            .insert("scanned".into(), scanned_files as i64);
        Ok(result)
    }
}

fn looks_like_definition(line: &str) -> bool {
    let t = line.trim_start();
    [
        "fn ",
        "pub fn ",
        "async fn ",
        "def ",
        "class ",
        "struct ",
        "enum ",
        "trait ",
        "impl ",
        "interface ",
        "func ",
        "type ",
        "const ",
        "export ",
    ]
    .iter()
    .any(|p| t.starts_with(p) || t.contains(&format!(" {p}")))
}

fn path_boost(rel: &str, pat_lc: &str) -> i32 {
    let r = rel.to_ascii_lowercase();
    let mut b = 0;
    if let Some(name) = r.rsplit('/').next() {
        if name.contains(pat_lc) {
            b += 2;
        }
    }
    if r.contains(pat_lc) {
        b += 1;
    }
    if r.starts_with("src/") {
        b += 1;
    }
    b
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(SearchTool));
}
