use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use wbash_core::args::parse_args;
use wbash_core::error::{Result, WErr};
use wbash_core::registry::{Tool, ToolContext};
use wbash_core::types::{Diagnostic, Severity, TestResult, Tier, ToolResult};

use crate::run::execute_with_parsers;
use crate::util;

pub struct CheckTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckArgs {
    #[serde(default = "all")]
    kind: String,
    #[serde(default = "dot")]
    path: String,
}

fn all() -> String {
    "all".to_string()
}
fn dot() -> String {
    ".".to_string()
}

impl Tool for CheckTool {
    fn name(&self) -> &'static str {
        "check"
    }
    fn description(&self) -> &'static str {
        "Detect and run build/lint/type/test checks; normalized diagnostics."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["all", "build", "lint", "types", "test"], "default": "all"},
                "path": {"type": "string", "default": "."}
            },
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: CheckArgs = parse_args(
            args.clone(),
            "check",
            "pass kind=all|build|lint|types|test and optional path",
        )?;
        if !matches!(a.kind.as_str(), "all" | "build" | "lint" | "types" | "test") {
            return Err(WErr::invalid_args(
                format!("unknown kind '{}'", a.kind),
                "use all|build|lint|types|test",
            ));
        }
        let target = ctx.resolve(&a.path, true)?;
        let base = if target.is_dir() {
            target.clone()
        } else {
            target.parent().unwrap_or(ctx.root).to_path_buf()
        };
        let project = detect_root(ctx.root, &base).unwrap_or_else(|| base.clone());
        let mut commands = detect_commands(&project, &a.kind)?;
        commands.dedup();
        if commands.is_empty() {
            return Err(WErr::with_hint(
                "no_checker_detected",
                format!("no checker detected under {}", ctx.rel(&project)),
                "add a Cargo.toml, package.json, pyproject.toml/setup.cfg or go.mod, or run the tool directly with `w run`",
            ));
        }

        let mut result = ToolResult::new("check", String::new());
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let mut tests: Vec<TestResult> = Vec::new();
        let mut all_ok = true;
        let mut ran = 0usize;
        for (label, argv) in &commands {
            let run = match execute_with_parsers(
                ctx,
                argv,
                &project,
                ctx.config.exec.timeout_ms,
                None,
                None,
                true,
            ) {
                Ok(r) => r,
                Err(e) if e.code == "exec_not_found" => {
                    result.note(format!("missing {label}"));
                    continue;
                }
                Err(e) => return Err(e),
            };
            ran += 1;
            all_ok &= run.output.exit.is_success();
            result.meta.raw_bytes = Some(
                result.meta.raw_bytes.unwrap_or(0)
                    + run.output.stdout.len() as u64
                    + run.output.stderr.len() as u64,
            );
            if run.output.timed_out {
                result.note(format!("timeout {label}"));
            }
            if let Some(p) = run.parsed {
                all_ok &= p.diagnostics.iter().all(|d| d.severity != Severity::Error)
                    && !p.tests.iter().any(|t| t.status.is_failure());
                diagnostics.extend(p.diagnostics);
                tests.extend(p.tests);
            } else if !run.output.stdout.is_empty() || !run.output.stderr.is_empty() {
                let raw = run.output.combined();
                if !run.output.exit.is_success() {
                    result.note(format!("{label} parse_miss raw_tail"));
                } else {
                    result.note(format!("{label} parse_fallback"));
                }
                util::compact_tail(&mut result, &raw, "check", ctx.handles, true)?;
            }
            if run.output.truncated {
                result.note(format!("{label} output_truncated"));
            }
        }
        if ran == 0 {
            return Err(WErr::with_hint(
                "no_checker_detected",
                "detected checkers are not installed",
                "install the toolchain or run the checker directly with `w run`",
            ));
        }

        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warn)
            .count();
        let (passed, failed, _) = util::counts_of(&tests);
        let mut header = format!("{errors} errors, {warnings} warnings");
        if failed > 0 {
            header.push_str(&format!(", {failed} failed"));
        }
        if passed > 0 {
            header.push_str(&format!(", {passed} passed"));
        }
        result.header = header;
        util::diagnostics_into(&mut result, diagnostics, ctx.root, ctx.handles)?;
        util::tests_into(&mut result, &tests, true);
        result.meta.counts.insert("errors".into(), errors as i64);
        result
            .meta
            .counts
            .insert("warnings".into(), warnings as i64);
        if failed > 0 {
            result.meta.counts.insert("failed".into(), failed as i64);
        }
        if passed > 0 {
            result.meta.counts.insert("passed".into(), passed as i64);
        }
        result.meta.ok = all_ok && errors == 0 && failed == 0;
        Ok(result)
    }
}

fn detect_root(root: &Path, start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        for marker in [
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "setup.cfg",
            "go.mod",
        ] {
            if cur.join(marker).exists() {
                return Some(cur);
            }
        }
        if cur == root {
            return None;
        }
        if !cur.pop() {
            return None;
        }
        if !cur.starts_with(root) {
            return None;
        }
    }
}

fn detect_commands(project: &Path, kind: &str) -> Result<Vec<(String, Vec<String>)>> {
    let mut out = Vec::new();
    let want = |k: &str| kind == "all" || kind == k;
    let s = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();

    if project.join("Cargo.toml").exists() {
        if want("build") || want("types") {
            out.push(("cargo check".to_string(), s(&["cargo", "check"])));
        }
        if want("lint") {
            out.push((
                "cargo clippy".to_string(),
                s(&["cargo", "clippy", "--all-targets"]),
            ));
        }
        if want("test") {
            out.push(("cargo test".to_string(), s(&["cargo", "test"])));
        }
    }

    if project.join("go.mod").exists() {
        if want("build") || want("types") {
            out.push(("go build".to_string(), s(&["go", "build", "./..."])));
        }
        if want("lint") {
            out.push(("go vet".to_string(), s(&["go", "vet", "./..."])));
        }
        if want("test") {
            out.push(("go test".to_string(), s(&["go", "test", "./..."])));
        }
    }

    if project.join("pyproject.toml").exists() || project.join("setup.cfg").exists() {
        if want("test") {
            if let Some(bin) = find_executable("pytest", project) {
                out.push(("pytest".to_string(), vec![bin, "-q".to_string()]));
            }
        }
        if want("lint") {
            if let Some(bin) = find_executable("ruff", project) {
                out.push((
                    "ruff".to_string(),
                    vec![bin, "check".to_string(), ".".to_string()],
                ));
            }
        }
        if want("types") {
            if let Some(bin) = find_executable("mypy", project) {
                out.push(("mypy".to_string(), vec![bin, ".".to_string()]));
            }
        }
    }

    if project.join("package.json").exists() {
        let bin = |name: &str| find_executable(name, project);
        if want("types") && project.join("tsconfig.json").exists() {
            if let Some(tsc) = bin("tsc") {
                out.push(("tsc".to_string(), vec![tsc, "--noEmit".to_string()]));
            }
        }
        if want("lint") {
            if let Some(eslint) = bin("eslint") {
                out.push(("eslint".to_string(), vec![eslint, ".".to_string()]));
            } else if has_script(project, "lint") {
                out.push(("npm lint".to_string(), s(&["npm", "run", "lint"])));
            }
        }
        if want("test") {
            if let Some(vitest) = bin("vitest") {
                out.push(("vitest".to_string(), vec![vitest, "run".to_string()]));
            } else if let Some(jest) = bin("jest") {
                out.push(("jest".to_string(), vec![jest]));
            } else if has_script(project, "test") {
                out.push(("npm test".to_string(), s(&["npm", "test"])));
            }
        }
        if want("build") && has_script(project, "build") {
            out.push(("npm build".to_string(), s(&["npm", "run", "build"])));
        }
    }

    let mut seen = BTreeSet::new();
    out.retain(|(_, argv)| seen.insert(argv.join("\u{0}")));
    Ok(out)
}

fn has_script(project: &Path, name: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(project.join("package.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    v.get("scripts").and_then(|s| s.get(name)).is_some()
}

fn find_executable(name: &str, project: &Path) -> Option<String> {
    let local = [
        project.join("node_modules").join(".bin").join(name),
        project.join(".venv").join("bin").join(name),
        project.join("venv").join("bin").join(name),
    ];
    for p in local {
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(CheckTool));
}
