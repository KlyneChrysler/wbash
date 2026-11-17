use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use wbash_core::args::{parse_args, parse_line_range};
use wbash_core::error::{Result, WErr};
use wbash_core::policy;
use wbash_core::registry::{Tool, ToolContext};
use wbash_core::types::{format_duration_ms, Tier, ToolResult};

use crate::run::execute_with_parsers;

pub struct GitTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitArgs {
    op: String,
    pathspec: Option<Value>,
    #[serde(default)]
    staged: bool,
    context: Option<u32>,
    limit: Option<u32>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    path: Option<String>,
    lines: Option<String>,
    message: Option<String>,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    allow_empty: bool,
    remote: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    set_upstream: bool,
}

impl Tool for GitTool {
    fn name(&self) -> &'static str {
        "git"
    }
    fn description(&self) -> &'static str {
        "Git read ops plus gated commit/push; compact deterministic output."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "op": {"type": "string", "enum": ["status", "diff", "log", "show", "blame", "branch", "commit", "push"]},
                "pathspec": {"type": ["string", "array"], "items": {"type": "string"}},
                "staged": {"type": "boolean", "default": false},
                "context": {"type": "integer", "minimum": 0, "maximum": 20},
                "limit": {"type": "integer", "minimum": 1},
                "ref": {"type": "string"},
                "path": {"type": "string"},
                "lines": {"type": "string"},
                "message": {"type": "string"},
                "all": {"type": "boolean", "default": false},
                "allow_empty": {"type": "boolean", "default": false},
                "remote": {"type": "string"},
                "branch": {"type": "string"},
                "set_upstream": {"type": "boolean", "default": false}
            },
            "required": ["op"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: GitArgs = parse_args(
            args.clone(),
            "git",
            "op=status|diff|log|show|blame|branch|commit|push",
        )?;
        ensure_repo(ctx)?;
        match a.op.as_str() {
            "status" => status(ctx),
            "diff" => diff(ctx, &a),
            "log" => log(ctx, &a),
            "show" => show(ctx, &a),
            "blame" => blame(ctx, &a),
            "branch" => branch(ctx),
            "commit" => commit(ctx, &a),
            "push" => push(ctx, &a),
            other => Err(WErr::with_hint(
                "invalid_args",
                format!("unknown git op '{other}'"),
                "use status|diff|log|show|blame|branch|commit|push",
            )),
        }
    }
}

fn git(ctx: &ToolContext<'_>, args: &[&str]) -> Result<wbash_core::executor::ExecOutput> {
    let argv: Vec<String> = std::iter::once("git".to_string())
        .chain(args.iter().map(|s| s.to_string()))
        .collect();
    let run = execute_with_parsers(
        ctx,
        &argv,
        ctx.root,
        ctx.config.exec.timeout_ms,
        None,
        None,
        false,
    )?;
    Ok(run.output)
}

fn ensure_repo(ctx: &ToolContext<'_>) -> Result<()> {
    let out = git(ctx, &["rev-parse", "--git-dir"])?;
    if !out.exit.is_success() {
        return Err(WErr::with_hint(
            "not_a_repo",
            "not a git repository",
            "run git init first or set --root to the repository",
        ));
    }
    Ok(())
}

fn status(ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
    let out = git(
        ctx,
        &["status", "--porcelain=v1", "--branch", "--no-renames"],
    )
    .map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("status", &out));
    }
    let text = out.stdout_str();
    let mut branch_name = "(detached)".to_string();
    let mut tracking = String::new();
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let name = if let Some(n) = rest.strip_prefix("No commits yet on ") {
                n
            } else if let Some(n) = rest.strip_prefix("Initial commit on ") {
                n
            } else {
                rest.split("...")
                    .next()
                    .unwrap_or(rest)
                    .split(' ')
                    .next()
                    .unwrap_or(rest)
            };
            branch_name = name.to_string();
            if let (Some(l), Some(r)) = (rest.find('['), rest.find(']')) {
                if r > l {
                    tracking = format!(" {}", &rest[l..=r]);
                }
            }
        } else if line.len() > 3 {
            let code = line[..2].trim().to_string();
            let path = line[3..].trim().to_string();
            entries.push((code, path));
        }
    }
    let mut r = ToolResult::new(
        "git",
        format!("branch {branch_name}{tracking}, {} changed", entries.len()),
    );
    for (code, path) in &entries {
        let class = if code.contains('U') || code == "AA" || code == "DD" {
            wbash_core::types::ItemClass::MustKeep
        } else {
            wbash_core::types::ItemClass::Low
        };
        r.push(wbash_core::types::Item::new(
            class,
            3.0,
            vec![format!("{code} {path}")],
        ));
    }
    r.meta.counts.insert("changed".into(), entries.len() as i64);
    Ok(r)
}

fn diff(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let context = a.context.unwrap_or(3);
    if context > ctx.config.search.context_max {
        return Err(WErr::invalid_args(
            format!(
                "context {context} exceeds max {}",
                ctx.config.search.context_max
            ),
            "context=3",
        ));
    }
    let mut argv: Vec<String> = vec![
        "diff".to_string(),
        "--no-color".to_string(),
        format!("--unified={context}"),
    ];
    if a.staged {
        argv.push("--cached".to_string());
    }
    let paths = pathspecs(a)?;
    if !paths.is_empty() {
        argv.push("--".to_string());
        argv.extend(paths);
    }
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("diff", &out));
    }
    let mut r = ToolResult::new("git", String::new());
    let mut files = 0usize;
    let mut plus = 0usize;
    let mut minus = 0usize;
    let mut current = String::new();
    for line in out.stdout_str().lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.rsplit(" b/").next().unwrap_or(rest).trim().to_string();
            current = path;
            files += 1;
            continue;
        }
        if line.starts_with("@@") {
            r.high(format!("{current} {line}"));
        } else if line.starts_with("+++") || line.starts_with("---") {
            continue;
        } else if let Some(rest) = line.strip_prefix('+') {
            plus += 1;
            r.low(format!("+{rest}"));
        } else if let Some(rest) = line.strip_prefix('-') {
            minus += 1;
            r.low(format!("-{rest}"));
        }
    }
    r.header = format!(
        "{} changed, +{plus} -{minus}",
        crate::util::plural(files, "file")
    );
    if out.truncated {
        r.note("output_truncated");
    }
    r.meta.counts.insert("files".into(), files as i64);
    r.meta.counts.insert("insertions".into(), plus as i64);
    r.meta.counts.insert("deletions".into(), minus as i64);
    Ok(r)
}

fn log(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let limit = a.limit.unwrap_or(20);
    let limit_s = format!("-n{limit}");
    let mut args: Vec<String> = vec![
        "log".to_string(),
        "--no-color".to_string(),
        "--date=short".to_string(),
        "--pretty=format:%h%x09%ad%x09%s".to_string(),
        limit_s,
    ];
    if let Some(r) = &a.reference {
        args.push(r.clone());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("log", &out));
    }
    let stdout = out.stdout_str();
    let lines: Vec<&str> = stdout.lines().collect();
    let mut r = ToolResult::new("git", format!("{} commits", lines.len()));
    for line in &lines {
        r.low(line.replace('\t', " "));
    }
    r.meta.counts.insert("commits".into(), lines.len() as i64);
    Ok(r)
}

fn show(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let reference = a
        .reference
        .clone()
        .ok_or_else(|| WErr::invalid_args("show requires ref", "ref=HEAD"))?;
    let mut args: Vec<String> = vec![
        "show".to_string(),
        "--no-color".to_string(),
        "--stat".to_string(),
        "--pretty=format:%h%x09%ad%x09%an%x09%s".to_string(),
        "--date=short".to_string(),
        reference.clone(),
    ];
    let paths = pathspecs(a)?;
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("show", &out));
    }
    let mut r = ToolResult::new("git", format!("show {reference}"));
    for line in out.stdout_str().lines() {
        if line.starts_with("diff --git") || line.starts_with("@@") {
            r.high(line.to_string());
        } else {
            r.low(line.to_string());
        }
    }
    if out.truncated {
        r.note("output_truncated");
    }
    Ok(r)
}

fn blame(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let path = a
        .path
        .clone()
        .ok_or_else(|| WErr::invalid_args("blame requires path", "path=src/main.rs"))?;
    let abs = ctx.resolve(&path, true)?;
    let display = ctx.rel(&abs);
    let mut args: Vec<String> = vec!["blame".to_string(), "--line-porcelain".to_string()];
    if let Some(spec) = &a.lines {
        let len = std::fs::read_to_string(&abs)
            .map(|s| s.lines().count())
            .unwrap_or(1);
        let (start, end) = parse_line_range(spec, len)?;
        args.push(format!("-L{start},{end}"));
    }
    args.push("--".to_string());
    args.push(display.clone());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("blame", &out));
    }
    let mut r = ToolResult::new("git", format!("blame {display}"));
    let mut sha = String::new();
    let mut author = String::new();
    let mut final_line = String::new();
    let mut count = 0usize;
    for line in out.stdout_str().lines() {
        if line.starts_with('\t') {
            count += 1;
            r.low(format!(
                "{final_line} {} {}: {}",
                &sha[..sha.len().min(8)],
                author,
                line.trim_start_matches('\t')
            ));
            continue;
        }
        let mut parts = line.split(' ');
        if let Some(first) = parts.next() {
            if first.len() >= 8 && first.chars().all(|c| c.is_ascii_hexdigit()) {
                sha = first.to_string();
                if let Some(n) = parts.nth(1) {
                    final_line = n.to_string();
                }
                continue;
            }
        }
        if let Some(v) = line.strip_prefix("author ") {
            author = v.to_string();
        }
    }
    r.meta.counts.insert("lines".into(), count as i64);
    Ok(r)
}

fn branch(ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
    let out = git(
        ctx,
        &[
            "branch",
            "--no-color",
            "--format=%(HEAD)%(refname:short)%09%(upstream:short)",
        ],
    )
    .map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("branch", &out));
    }
    let stdout = out.stdout_str();
    let lines: Vec<&str> = stdout.lines().collect();
    let mut r = ToolResult::new("git", format!("{} branches", lines.len()));
    for line in &lines {
        let (marker, rest) = line.split_at(1.min(line.len()));
        let mut parts = rest.split('\t');
        let name = parts.next().unwrap_or("");
        let upstream = parts.next().unwrap_or("");
        let current = marker == "*";
        let line = if upstream.is_empty() {
            format!("{} {name}", if current { "*" } else { " " })
        } else {
            format!("{} {name} -> {upstream}", if current { "*" } else { " " })
        };
        let class = if current {
            wbash_core::types::ItemClass::High
        } else {
            wbash_core::types::ItemClass::Low
        };
        r.push(wbash_core::types::Item::new(
            class,
            if current { 8.0 } else { 3.0 },
            vec![line],
        ));
    }
    r.meta.counts.insert("branches".into(), lines.len() as i64);
    Ok(r)
}

fn commit(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git commit")?;
    let message = a
        .message
        .clone()
        .ok_or_else(|| WErr::invalid_args("commit requires message", "message=<commit subject>"))?;
    let mut args: Vec<String> = vec!["commit".to_string(), "-m".to_string(), message];
    if a.all {
        args.push("-a".to_string());
    }
    if a.allow_empty {
        args.push("--allow-empty".to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    let mut r = ToolResult::new(
        "git",
        format!("commit, {}", format_duration_ms(out.duration_ms)),
    );
    if out.exit.is_success() {
        if let Ok(head) = git(ctx, &["rev-parse", "--short", "HEAD"]) {
            let sha = head.stdout_str().trim().to_string();
            if !sha.is_empty() {
                r.header = format!("committed {sha}, {}", format_duration_ms(out.duration_ms));
            }
        }
    } else {
        r.meta.ok = false;
    }
    let text = out.combined();
    if !text.trim().is_empty() {
        let h = ctx.handles.put("git", text.as_bytes(), "text")?;
        r.handle(wbash_core::types::HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
        r.must(format!("full: {}", h.id));
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        r.meta.counts.insert("output_lines".into(), lines as i64);
    }
    r.meta.exit = Some(out.exit);
    Ok(r)
}

fn push(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git push")?;
    let mut args: Vec<String> = vec!["push".to_string()];
    if let Some(remote) = &a.remote {
        args.push(remote.clone());
    }
    if let Some(branch) = &a.branch {
        args.push(branch.clone());
    }
    if a.set_upstream {
        args.push("--set-upstream".to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    let first = out
        .stdout_str()
        .lines()
        .chain(out.stderr_str().lines())
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| "push".to_string());
    let mut r = ToolResult::new(
        "git",
        format!("push, {} {first}", format_duration_ms(out.duration_ms)),
    );
    r.meta.ok = out.exit.is_success();
    r.meta.exit = Some(out.exit);
    let text = out.combined();
    let h = ctx.handles.put("git", text.as_bytes(), "text")?;
    r.handle(wbash_core::types::HandleRef {
        id: h.id.clone(),
        kind: h.kind.clone(),
        bytes: h.bytes,
        hash: h.hash.clone(),
    });
    r.must(format!("full: {}", h.id));
    Ok(r)
}

fn pathspecs(a: &GitArgs) -> Result<Vec<String>> {
    match &a.pathspec {
        None => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(s) => out.push(s.to_string()),
                    None => {
                        return Err(WErr::invalid_args(
                            "pathspec must be strings",
                            "pathspec=\"src/\"",
                        ))
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(WErr::invalid_args(
            "pathspec must be a string or array",
            "pathspec=\"src/\"",
        )),
    }
}

fn map_git_err(err: WErr) -> WErr {
    if err.code == "exec_not_found" {
        WErr::with_hint(
            "git_error",
            "git executable not found",
            "install git and ensure it is on PATH",
        )
    } else {
        err
    }
}

fn git_failure(op: &str, out: &wbash_core::executor::ExecOutput) -> WErr {
    let detail = out
        .stderr_str()
        .lines()
        .chain(out.stdout_str().lines())
        .find(|l| !l.trim().is_empty())
        .unwrap_or("git failed")
        .trim()
        .to_string();
    WErr::with_hint(
        "git_error",
        format!("{op}: {detail}"),
        "inspect with `w git status`",
    )
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(GitTool));
}

pub fn _paths(_: &Path) -> Option<PathBuf> {
    None
}
