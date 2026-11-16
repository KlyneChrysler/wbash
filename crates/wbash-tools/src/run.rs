use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use wbash_core::args::parse_args;
use wbash_core::error::{Result, WErr};
use wbash_core::executor::{self, ExecOutput};
use wbash_core::parsers::ParsedRun;
use wbash_core::registry::{Tool, ToolContext};
use wbash_core::types::{format_duration_ms, Exit, HandleRef, Tier, ToolResult};

use crate::util;

pub struct ExecRun {
    pub output: ExecOutput,
    pub parsed: Option<ParsedRun>,
    pub injected: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn execute_with_parsers(
    ctx: &ToolContext<'_>,
    argv: &[String],
    cwd: &Path,
    timeout_ms: u64,
    stdin: Option<&[u8]>,
    env: Option<&HashMap<String, String>>,
    inject: bool,
) -> Result<ExecRun> {
    if argv.is_empty() {
        return Err(WErr::invalid_args(
            "argv must not be empty",
            "argv=[\"prog\", \"arg\"]",
        ));
    }
    if timeout_ms == 0 || timeout_ms > ctx.config.exec.max_timeout_ms {
        return Err(WErr::invalid_args(
            format!("timeout_ms {timeout_ms} out of range"),
            format!("timeout_ms must be 1..={}", ctx.config.exec.max_timeout_ms),
        ));
    }
    let injected = if inject {
        ctx.parsers.inject_args(argv)
    } else {
        argv.to_vec()
    };
    let env_map = executor::build_env(ctx.config, env)?;
    let req = executor::ExecRequest {
        argv: injected.clone(),
        cwd: cwd.to_path_buf(),
        timeout_ms,
        stdin: stdin.map(|s| s.to_vec()),
        env: env_map,
        max_output_bytes: ctx.config.exec.max_output_bytes,
    };
    let output = executor::execute(&req)?;
    let parsed = if inject {
        ctx.parsers.parse(
            &injected,
            &output.exit,
            &output.stdout_str(),
            &output.stderr_str(),
        )
    } else {
        None
    };
    Ok(ExecRun {
        output,
        parsed,
        injected,
    })
}

pub fn record_output(
    result: &mut ToolResult,
    ctx: &mut ToolContext<'_>,
    output: &ExecOutput,
    parsed: Option<&ParsedRun>,
    label: &str,
    inject_full: bool,
) -> Result<()> {
    let raw = output.combined();
    result.meta.raw_bytes = Some(raw.len() as u64);
    result.meta.duration_ms = Some(output.duration_ms);
    result.meta.exit = Some(output.exit);
    result.meta.ok = output.exit.is_success();

    if output.truncated {
        result.note("output_truncated");
    }
    if output.timed_out {
        result.note("timed_out");
    }

    result.raw_text = Some(raw.clone());
    let mut full_ref: Option<String> = None;
    let parse_miss_pre = if let Some(p) = parsed {
        let (_, failed, _) = util::counts_of(&p.tests);
        let errors = p
            .diagnostics
            .iter()
            .filter(|d| d.severity == wbash_core::types::Severity::Error)
            .count();
        !output.exit.is_success() && errors == 0 && failed == 0 && p.tests.is_empty()
    } else {
        !output.exit.is_success()
    } && !raw.trim().is_empty();
    let need_handle =
        (inject_full || raw.len() > 4096 || parsed.is_some() || !output.exit.is_success())
            && !parse_miss_pre;
    if need_handle {
        let h = ctx.handles.put(label, raw.as_bytes(), "text")?;
        result.handle(HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
        full_ref = Some(h.id);
    }

    if let Some(p) = parsed {
        let (passed, failed, _skipped) = util::counts_of(&p.tests);
        let errors = p
            .diagnostics
            .iter()
            .filter(|d| d.severity == wbash_core::types::Severity::Error)
            .count();
        let warnings = p
            .diagnostics
            .iter()
            .filter(|d| d.severity == wbash_core::types::Severity::Warn)
            .count();
        let parse_miss =
            !output.exit.is_success() && errors == 0 && failed == 0 && p.tests.is_empty();
        if let Some(summary) = &p.summary {
            if !summary.is_empty() {
                result.header.push_str(", ");
                result.header.push_str(summary);
            }
        }
        let diags = p.diagnostics.clone();
        util::diagnostics_into(result, diags, ctx.root, ctx.handles)?;
        util::tests_into(result, &p.tests, true);
        if passed > 0 {
            result.meta.counts.insert("passed".into(), passed as i64);
        }
        if failed > 0 {
            result.meta.counts.insert("failed".into(), failed as i64);
        }
        if errors > 0 {
            result.meta.counts.insert("errors".into(), errors as i64);
        }
        if warnings > 0 {
            result
                .meta
                .counts
                .insert("warnings".into(), warnings as i64);
        }
        if parse_miss && !raw.trim().is_empty() {
            result.note("parse_miss raw_tail");
            util::compact_tail(result, &raw, label, ctx.handles, true)?;
        }
    } else if !raw.trim().is_empty() {
        if !output.exit.is_success() {
            result.note("parse_miss raw_tail");
            util::compact_tail(result, &raw, label, ctx.handles, true)?;
        } else {
            for line in raw.lines() {
                let line = line.trim_end();
                if !line.is_empty() {
                    result.low(line.to_string());
                }
            }
        }
    }

    if let Some(id) = full_ref {
        let noisy = result
            .items
            .iter()
            .any(|i| i.class == wbash_core::types::ItemClass::Low && i.lines.len() > 40);
        if (!output.exit.is_success() || noisy)
            && !result
                .items
                .iter()
                .any(|i| i.lines.iter().any(|l| l == &format!("full: {id}")))
        {
            result.must(format!("full: {id}"));
        }
    }
    Ok(())
}

fn exit_text(exit: &Exit) -> String {
    match exit {
        Exit::Code(c) => format!("exit {c}"),
        Exit::Timeout => "exit timeout".to_string(),
    }
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

pub struct RunTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    argv: Vec<String>,
    #[serde(default = "dot")]
    cwd: String,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
    env: Option<HashMap<String, String>>,
}

fn dot() -> String {
    ".".to_string()
}

impl Tool for RunTool {
    fn name(&self) -> &'static str {
        "run"
    }
    fn description(&self) -> &'static str {
        "Execute argv directly, normalize output, keep full log in a handle."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
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
                "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1},
                "cwd": {"type": "string", "default": "."},
                "timeout_ms": {"type": "integer", "minimum": 1},
                "stdin": {"type": "string"},
                "env": {"type": "object", "additionalProperties": {"type": "string"}}
            },
            "required": ["argv"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: RunArgs = parse_args(
            args.clone(),
            "run",
            "pass argv=[\"cmd\", \"args\"] plus optional cwd, timeout_ms, stdin, env",
        )?;
        let cwd = ctx.resolve(&a.cwd, true)?;
        if !cwd.is_dir() {
            return Err(WErr::new(
                "cwd_not_found",
                format!("cwd '{}' is not a directory", a.cwd),
            ));
        }
        let timeout = a.timeout_ms.unwrap_or(ctx.config.exec.timeout_ms);
        let run = execute_with_parsers(
            ctx,
            &a.argv,
            &cwd,
            timeout,
            a.stdin.as_ref().map(|s| s.as_bytes()),
            a.env.as_ref(),
            true,
        )?;
        let header = if run.output.duration_ms >= 1000 {
            format!(
                "{}, {}",
                exit_text(&run.output.exit),
                format_duration_ms(run.output.duration_ms)
            )
        } else {
            exit_text(&run.output.exit)
        };
        let mut result = ToolResult::new("run", header);
        record_output(
            &mut result,
            ctx,
            &run.output,
            run.parsed.as_ref(),
            "run",
            true,
        )?;
        let _ = run.injected;
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// sh (escape hatch)
// ---------------------------------------------------------------------------

pub struct ShTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShArgs {
    command: String,
    #[serde(default = "dot")]
    cwd: String,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
}

impl Tool for ShTool {
    fn name(&self) -> &'static str {
        "sh"
    }
    fn description(&self) -> &'static str {
        "Run a shell command (requires allow_shell); generic compaction."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
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
                "command": {"type": "string"},
                "cwd": {"type": "string", "default": "."},
                "timeout_ms": {"type": "integer", "minimum": 1},
                "stdin": {"type": "string"}
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: ShArgs = parse_args(
            args.clone(),
            "sh",
            "pass command plus optional cwd, timeout_ms, stdin",
        )?;
        let cwd = ctx.resolve(&a.cwd, true)?;
        let timeout = a.timeout_ms.unwrap_or(ctx.config.exec.timeout_ms);
        let argv = vec!["/bin/sh".to_string(), "-lc".to_string(), a.command.clone()];
        let run = execute_with_parsers(
            ctx,
            &argv,
            &cwd,
            timeout,
            a.stdin.as_ref().map(|s| s.as_bytes()),
            None,
            false,
        )?;
        let header = if run.output.duration_ms >= 1000 {
            format!(
                "{}, {}",
                exit_text(&run.output.exit),
                format_duration_ms(run.output.duration_ms)
            )
        } else {
            exit_text(&run.output.exit)
        };
        let mut result = ToolResult::new("sh", header);
        record_output(&mut result, ctx, &run.output, None, "sh", true)?;
        result.meta.counts.insert("shell".into(), 1);
        Ok(result)
    }
}

pub fn path_or_root(ctx: &ToolContext<'_>, cwd: &Path) -> PathBuf {
    if cwd.is_dir() {
        cwd.to_path_buf()
    } else {
        ctx.root.to_path_buf()
    }
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(RunTool));
    out.push(std::sync::Arc::new(ShTool));
}
