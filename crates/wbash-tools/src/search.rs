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

        let walk = ctx.workspace.walk_filtered(Some(&|rel| {
            util::under(rel, &base_rel) && util::matches_glob(gs.as_ref(), rel)
        }));
        for f in &walk.files {
            scanned_files += 1;
            let Ok(text) = wbash_index::read_text(&f.abs) else {
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
                    if s.name == a.pattern
                        || s.name
                            .to_ascii_lowercase()
                            .contains(&a.pattern.to_ascii_lowercase())
                    {
                        hits.push(Hit {
                            path: f.rel.clone(),
                            line: s.start_line as u32,
                            col: 1,
                            snippet: format!("{} {}", s.kind, s.line_text),
                            is_definition: true,
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
                    hits.push(Hit {
                        path: f.rel.clone(),
                        line: (idx + 1) as u32,
                        col: col as u32,
                        snippet,
                        is_definition: looks_like_definition(line),
                    });
                }
            }
            if hits.len() > before {
                files_with_hits += 1;
            }
        }

        hits.sort_by(|a, b| {
            b.is_definition
                .cmp(&a.is_definition)
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

        let mut current_path = String::new();
        let mut context_lines: Vec<String> = Vec::new();
        for hit in &hits {
            if hit.path != current_path {
                current_path = hit.path.clone();
                context_lines.clear();
            }
            let class = if hit.is_definition {
                ItemClass::High
            } else {
                ItemClass::Low
            };
            result.push(Item::new(
                class,
                if hit.is_definition { 8.0 } else { 3.0 },
                vec![format!("{}:{}: {}", hit.path, hit.line, hit.snippet)],
            ));
            if a.context > 0 {
                if let Ok(t) = wbash_index::read_text(&ctx.root.join(&hit.path)) {
                    let all: Vec<&str> = t.content.lines().collect();
                    let lo = (hit.line as usize).saturating_sub(a.context as usize + 1);
                    let hi = ((hit.line as usize) + a.context as usize).min(all.len());
                    for (i, l) in all.iter().enumerate().take(hi).skip(lo) {
                        let n = i + 1;
                        if n as u32 == hit.line {
                            continue;
                        }
                        context_lines.push(format!("{}:{}- {}", hit.path, n, l.trim()));
                    }
                }
            }
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
        "interface ",
        "func ",
        "type ",
    ]
    .iter()
    .any(|p| t.starts_with(p) || t.contains(&format!(" {p}")))
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(SearchTool));
}
