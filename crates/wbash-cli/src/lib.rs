use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use wbash_core::config;
use wbash_core::dispatcher::Dispatcher;
use wbash_core::error::{Result, WErr};
use wbash_core::parsers::NoParsers;
use wbash_core::stats;
use wbash_core::types::Tier;
use wbash_parsers::ParserRegistry;

pub struct Global {
    pub root: PathBuf,
    pub session: String,
    pub format: Option<String>,
    pub budget: Option<u64>,
    pub config: Option<PathBuf>,
    pub no_project_config: bool,
    pub tier: Option<String>,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            session: "default".to_string(),
            format: None,
            budget: None,
            config: None,
            no_project_config: false,
            tier: None,
        }
    }
}

const BOOLEANS: &[&str] = &[
    "outline",
    "sizes",
    "staged",
    "all",
    "allow-empty",
    "set-upstream",
    "create",
    "delete",
    "overwrite",
    "stats",
    "json",
    "quiet",
];

pub fn run(argv: Vec<String>) -> i32 {
    match run_inner(argv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", e.render());
            e.cli_exit()
        }
    }
}

fn run_inner(argv: Vec<String>) -> Result<i32> {
    if argv.is_empty() {
        print_help();
        return Ok(0);
    }
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("w {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    if argv[0] == "-h" || argv[0] == "--help" || argv[0] == "help" {
        print_help();
        return Ok(0);
    }

    let (global, tool, rest) = parse_global(&argv)?;
    let Some(tool) = tool else {
        print_help();
        return Ok(0);
    };

    if rest.iter().any(|a| a == "--help" || a == "-h") {
        let registry = wbash_tools::registry();
        if let Some(t) = registry.get(&tool) {
            println!("w {}: {}", t.name(), t.description());
            println!("tier: {}", t.min_tier());
            println!(
                "{}",
                serde_json::to_string_pretty(&t.schema()).unwrap_or_default()
            );
            return Ok(0);
        }
        print_help();
        return Ok(0);
    }

    match tool.as_str() {
        "gain" => return cmd_gain(&rest),
        "discover" => return cmd_discover(&rest),
        "parser" => return cmd_parser(&rest),
        "handles" => return cmd_handles(&rest, &global),
        "config" => return cmd_config(&rest, &global),
        _ => {}
    }

    if tool == "root" || tool == "help" {
        print_help();
        return Ok(0);
    }

    let (mut obj, positionals, after_dd) = parse_tool_args(&rest, &tool)?;
    if tool == "run" || tool == "sh" {
        if tool == "run" {
            let argv_pos: Vec<String> = positionals.clone();
            let argv = if argv_pos.is_empty() {
                after_dd.clone()
            } else {
                argv_pos
            };
            if !argv.is_empty() {
                obj.insert(
                    "argv".into(),
                    Value::Array(argv.into_iter().map(Value::String).collect()),
                );
            }
        } else if let Some(cmd) = positionals.first() {
            obj.insert("command".into(), Value::String(cmd.clone()));
        }
    } else if tool == "git" {
        build_git_args(&mut obj, &positionals)?;
    } else if tool == "search" {
        if !positionals.is_empty() {
            obj.insert("pattern".into(), Value::String(positionals[0].clone()));
        }
        if positionals.len() == 2 {
            obj.insert("path".into(), Value::String(positionals[1].clone()));
        } else if positionals.len() > 2 {
            let paths: Vec<Value> = positionals[1..]
                .iter()
                .cloned()
                .map(Value::String)
                .collect();
            obj.insert("paths".into(), Value::Array(paths));
        }
    } else {
        fill_positionals(&tool, &mut obj, &positionals)?;
    }

    if let Some(format) = &global.format {
        obj.insert("format".into(), Value::String(format.clone()));
    }
    if let Some(budget) = global.budget {
        obj.insert("budget".into(), Value::Number(budget.into()));
    }
    if let Some(path) = obj.get("calls_file").and_then(Value::as_str) {
        let text = std::fs::read_to_string(path)
            .map_err(|e| WErr::new("invalid_args", format!("calls_file '{path}': {e}")))?;
        let calls: Value = serde_json::from_str(&text).map_err(|e| {
            WErr::invalid_args(
                format!("calls_file '{path}': {e}"),
                "expected a JSON array of {tool,args}",
            )
        })?;
        obj.remove("calls_file");
        obj.insert("calls".into(), calls);
    }

    let mut dispatcher = build_dispatcher(&global, true)?;
    if let Some(tier) = &global.tier {
        let t: Tier = tier.parse()?;
        dispatcher.set_session_tier(&global.session, t)?;
    }
    let rendered = dispatcher.call(&tool, &Value::Object(obj), &global.session);
    let mut out = std::io::stdout().lock();
    out.write_all(rendered.text.as_bytes())
        .map_err(|e| WErr::internal(format!("stdout: {e}")))?;
    out.flush().ok();
    Ok(rendered.cli_code)
}

pub fn build_dispatcher(global: &Global, with_parsers: bool) -> Result<Dispatcher> {
    let (cfg, warnings) = config::load(
        &global.root,
        global.config.as_deref(),
        global.no_project_config,
    )?;
    for w in warnings {
        eprintln!("# {w}");
    }
    let registry = Arc::new(wbash_tools::registry());
    if with_parsers {
        Dispatcher::new(cfg, &global.root, registry, Arc::new(ParserRegistry::new()))
    } else {
        Dispatcher::new(cfg, &global.root, registry, Arc::new(NoParsers))
    }
}

fn parse_global(argv: &[String]) -> Result<(Global, Option<String>, Vec<String>)> {
    let mut g = Global::default();
    let mut i = 0usize;
    while i < argv.len() {
        let a = &argv[i];
        match a.as_str() {
            "--root" => {
                g.root = PathBuf::from(next(argv, &mut i, "--root")?);
            }
            "--session" => {
                g.session = next(argv, &mut i, "--session")?;
            }
            "--format" => {
                g.format = Some(next(argv, &mut i, "--format")?);
            }
            "--budget" => {
                let v = next(argv, &mut i, "--budget")?;
                g.budget = Some(v.parse::<u64>().map_err(|_| {
                    WErr::invalid_args(format!("bad --budget '{v}'"), "--budget 600")
                })?);
            }
            "--config" => {
                g.config = Some(PathBuf::from(next(argv, &mut i, "--config")?));
            }
            "--tier" => {
                g.tier = Some(next(argv, &mut i, "--tier")?);
            }
            "--no-project-config" => {
                g.no_project_config = true;
                i += 1;
            }
            "-q" | "--quiet" => {
                i += 1;
            }
            "-h" | "--help" => {
                return Ok((g, None, Vec::new()));
            }
            other if other.starts_with('-') => {
                return Err(WErr::invalid_args(
                    format!("unknown global option '{other}'"),
                    "globals: --root --session --format --budget --config --tier --no-project-config",
                ));
            }
            _ => {
                return Ok((g, Some(a.clone()), argv[i + 1..].to_vec()));
            }
        }
    }
    Ok((g, None, Vec::new()))
}

fn next(argv: &[String], i: &mut usize, flag: &str) -> Result<String> {
    if *i + 1 >= argv.len() {
        return Err(WErr::invalid_args(
            format!("{flag} requires a value"),
            format!("{flag} <value>"),
        ));
    }
    *i += 2;
    Ok(argv[*i - 1].clone())
}

fn kebab_to_snake(s: &str) -> String {
    s.replace('-', "_")
}

type ParsedToolArgs = (Map<String, Value>, Vec<String>, Vec<String>);

fn parse_tool_args(tokens: &[String], tool: &str) -> Result<ParsedToolArgs> {
    let mut obj = Map::new();
    let mut positionals = Vec::new();
    let mut after_dd = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == "--" {
            after_dd.extend(tokens[i + 1..].iter().cloned());
            break;
        }
        if tok == "-m" {
            i += 1;
            if i >= tokens.len() {
                return Err(WErr::invalid_args(
                    "-m requires a message",
                    "-m \"message\"",
                ));
            }
            insert_value(&mut obj, "message", Value::String(tokens[i].clone()));
            i += 1;
            continue;
        }
        if tok.starts_with("--no-") {
            let key = kebab_to_snake(tok.trim_start_matches("--no-"));
            insert_value(&mut obj, &key, Value::Bool(false));
            i += 1;
            continue;
        }
        if let Some(flag) = tok.strip_prefix("--") {
            let (raw_key, inline) = match flag.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (flag, None),
            };
            let key = kebab_to_snake(raw_key);
            if key == "env" {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        if i >= tokens.len() {
                            return Err(WErr::invalid_args(
                                "--env requires KEY=VALUE",
                                "--env KEY=VALUE",
                            ));
                        }
                        tokens[i].clone()
                    }
                };
                let (k, val) = v.split_once('=').ok_or_else(|| {
                    WErr::invalid_args(format!("bad --env '{v}'"), "--env KEY=VALUE")
                })?;
                let env = obj
                    .entry("env".to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Some(map) = env.as_object_mut() {
                    map.insert(k.to_string(), Value::String(val.to_string()));
                }
                i += 1;
                continue;
            }
            let is_bool = BOOLEANS.contains(&raw_key);
            let value = match inline {
                Some(v) => parse_scalar(&v),
                None if is_bool => {
                    if i + 1 < tokens.len() && (tokens[i + 1] == "true" || tokens[i + 1] == "false")
                    {
                        i += 1;
                        Value::Bool(tokens[i] == "true")
                    } else {
                        Value::Bool(true)
                    }
                }
                None => {
                    i += 1;
                    if i >= tokens.len() {
                        return Err(WErr::invalid_args(
                            format!("--{raw_key} requires a value"),
                            format!("--{raw_key} <value>"),
                        ));
                    }
                    parse_scalar(&tokens[i])
                }
            };
            insert_value(&mut obj, &key, value);
            i += 1;
            continue;
        }
        if !matches!(tool, "run" | "sh") {
            if let Some((raw_key, raw_value)) = tok.split_once('=') {
                let valid = !raw_key.is_empty()
                    && raw_key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                if valid && !raw_key.starts_with('-') {
                    let key = kebab_to_snake(raw_key);
                    insert_value(&mut obj, &key, parse_scalar(raw_value));
                    i += 1;
                    continue;
                }
            }
        }
        positionals.push(tok.clone());
        i += 1;
    }
    Ok((obj, positionals, after_dd))
}

fn parse_scalar(s: &str) -> Value {
    if s == "true" {
        return Value::Bool(true);
    }
    if s == "false" {
        return Value::Bool(false);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    if (s.starts_with('[') && s.ends_with(']')) || (s.starts_with('{') && s.ends_with('}')) {
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            return v;
        }
    }
    Value::String(s.to_string())
}

fn insert_value(obj: &mut Map<String, Value>, key: &str, value: Value) {
    match obj.get_mut(key) {
        None => {
            obj.insert(key.to_string(), value);
        }
        Some(Value::Array(arr)) => arr.push(value),
        Some(existing) => {
            let old = existing.clone();
            *existing = Value::Array(vec![old, value]);
        }
    }
}

fn fill_positionals(tool: &str, obj: &mut Map<String, Value>, pos: &[String]) -> Result<()> {
    let mut put = |key: &str, val: &str| {
        if !obj.contains_key(key) {
            obj.insert(key.to_string(), Value::String(val.to_string()));
        }
    };
    match tool {
        "tree" => {
            if let Some(p) = pos.first() {
                put("path", p);
            }
        }
        "read" | "blame" | "edit" => {
            if let Some(p) = pos.first() {
                put("path", p);
            }
        }
        "search" => {
            if let Some(p) = pos.first() {
                put("pattern", p);
            }
            if let Some(p) = pos.get(1) {
                put("path", p);
            }
        }
        "expand" => {
            if let Some(p) = pos.first() {
                put("ref", p);
            }
        }
        "check" => {
            if let Some(p) = pos.first() {
                put("path", p);
            }
        }
        _ => {
            if !pos.is_empty() {
                return Err(WErr::invalid_args(
                    format!("{tool}: unexpected positional '{}'", pos[0]),
                    format!("see `w {tool} --help`"),
                ));
            }
        }
    }
    Ok(())
}

fn build_git_args(obj: &mut Map<String, Value>, pos: &[String]) -> Result<()> {
    let Some(op) = pos
        .first()
        .cloned()
        .or_else(|| obj.get("op").and_then(Value::as_str).map(str::to_string))
    else {
        return Err(WErr::invalid_args(
            "git requires an op",
            "git status|diff|log|show|blame|branch|commit|push",
        ));
    };
    obj.insert("op".into(), Value::String(op.clone()));
    let rest = &pos[1.min(pos.len())..];
    match op.as_str() {
        "commit" if !obj.contains_key("message") && !rest.is_empty() => {
            obj.insert("message".into(), Value::String(rest.join(" ")));
        }
        "diff" if !rest.is_empty() && !obj.contains_key("pathspec") => {
            obj.insert(
                "pathspec".into(),
                Value::Array(rest.iter().cloned().map(Value::String).collect()),
            );
        }
        "show" => {
            if let Some(r) = rest.first() {
                if !obj.contains_key("ref") {
                    obj.insert("ref".into(), Value::String(r.clone()));
                }
            }
            if rest.len() > 1 && !obj.contains_key("pathspec") {
                obj.insert(
                    "pathspec".into(),
                    Value::Array(rest[1..].iter().cloned().map(Value::String).collect()),
                );
            }
        }
        "log" => {
            if let Some(r) = rest.first() {
                if !obj.contains_key("ref") {
                    obj.insert("ref".into(), Value::String(r.clone()));
                }
            }
        }
        "blame" => {
            if let Some(p) = rest.first() {
                if !obj.contains_key("path") {
                    obj.insert("path".into(), Value::String(p.clone()));
                }
            }
        }
        "push" => {
            if let Some(r) = rest.first() {
                if !obj.contains_key("remote") {
                    obj.insert("remote".into(), Value::String(r.clone()));
                }
            }
            if let Some(b) = rest.get(1) {
                if !obj.contains_key("branch") {
                    obj.insert("branch".into(), Value::String(b.clone()));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Utility commands
// ---------------------------------------------------------------------------

fn parse_since(tokens: &[String]) -> Option<i64> {
    let idx = tokens.iter().position(|t| t == "--since")?;
    let spec = tokens.get(idx + 1)?;
    since_ms(spec)
}

fn since_ms(spec: &str) -> Option<i64> {
    let now = wbash_core::types::now_ms();
    let s = spec.trim();
    if let Some(days) = s.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
        return Some(now - days * 86_400_000);
    }
    if let Some(hours) = s.strip_suffix('h').and_then(|n| n.parse::<i64>().ok()) {
        return Some(now - hours * 3_600_000);
    }
    if let Some(mins) = s.strip_suffix('m').and_then(|n| n.parse::<i64>().ok()) {
        return Some(now - mins * 60_000);
    }
    if let Some(days) = days_from_ymd(s) {
        return Some(days * 86_400_000);
    }
    None
}

fn days_from_ymd(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn cmd_gain(tokens: &[String]) -> Result<i32> {
    let json = tokens.iter().any(|t| t == "--json");
    let since = parse_since(tokens);
    let s = stats::gain(since);
    if json {
        println!("{}", serde_json::to_string(&s)?);
    } else {
        println!(
            "calls {}, out {} tokens, raw {} tokens, saved {} tokens",
            s.calls, s.out_tokens, s.raw_tokens, s.saved_tokens
        );
        if !s.by_tool.is_empty() {
            let tools: Vec<String> = s.by_tool.iter().map(|(k, v)| format!("{k} {v}")).collect();
            println!("by_tool {}", tools.join(", "));
        }
    }
    Ok(0)
}

fn cmd_discover(tokens: &[String]) -> Result<i32> {
    let json = tokens.iter().any(|t| t == "--json");
    let since = parse_since(tokens);
    let s = stats::discover(since);
    if json {
        println!("{}", serde_json::to_string(&s)?);
    } else {
        println!(
            "calls {}, sh {}, fallback {}",
            s.calls, s.sh_calls, s.fallback_calls
        );
        if !s.per_tool.is_empty() {
            let tools: Vec<String> = s.per_tool.iter().map(|(k, v)| format!("{k} {v}")).collect();
            println!("by_tool {}", tools.join(", "));
        }
    }
    Ok(0)
}

fn cmd_parser(tokens: &[String]) -> Result<i32> {
    if tokens.first().map(String::as_str) != Some("test") {
        return Err(WErr::invalid_args(
            "parser requires subcommand 'test'",
            "w parser test <parser.toml> <sample>",
        ));
    }
    let toml_path = tokens.get(1).ok_or_else(|| {
        WErr::invalid_args(
            "missing parser file",
            "w parser test <parser.toml> <sample>",
        )
    })?;
    let sample_path = tokens.get(2).ok_or_else(|| {
        WErr::invalid_args(
            "missing sample file",
            "w parser test <parser.toml> <sample>",
        )
    })?;
    let parser = wbash_parsers::TomlParser::load(Path::new(toml_path))?;
    let sample = std::fs::read_to_string(sample_path)
        .map_err(|e| WErr::new("not_found", format!("sample '{sample_path}': {e}")))?;
    let parsed = parser.parse(&sample, "");
    match parsed {
        Some(run) => {
            let out = json!({
                "parser": parser.name,
                "summary": run.summary,
                "tests": run.tests,
                "diagnostics": run.diagnostics,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(0)
        }
        None => Err(WErr::with_hint(
            "parse_failed",
            format!("parser '{}' produced no records", parser.name),
            "check [map.test_result] JSON paths and [parse].format",
        )),
    }
}

fn cmd_handles(tokens: &[String], global: &Global) -> Result<i32> {
    if tokens.first().map(String::as_str) != Some("gc") {
        return Err(WErr::invalid_args(
            "handles requires subcommand 'gc'",
            "w handles gc",
        ));
    }
    let mut dispatcher = build_dispatcher(global, false)?;
    dispatcher.open_session(&global.session)?;
    let removed = dispatcher.gc_handles(&global.session)?;
    println!("handles gc: {removed} removed");
    Ok(0)
}

fn cmd_config(tokens: &[String], global: &Global) -> Result<i32> {
    match tokens.first().map(String::as_str) {
        Some("show") => {
            let (cfg, warnings) = config::load(
                &global.root,
                global.config.as_deref(),
                global.no_project_config,
            )?;
            for w in warnings {
                eprintln!("# {w}");
            }
            print!("{}", config::render(&cfg));
            Ok(0)
        }
        Some("trust") => {
            config::trust_root(&global.root)?;
            let root = global
                .root
                .canonicalize()
                .unwrap_or_else(|_| global.root.clone());
            println!("trusted {}", root.display());
            Ok(0)
        }
        _ => Err(WErr::invalid_args(
            "config requires subcommand",
            "w config show | w config trust",
        )),
    }
}

fn print_help() {
    println!(
        "w - agent-native workspace tool server ({})",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "usage: w [--root DIR] [--session ID] [--format lean|json] [--budget N] <tool> [opts]"
    );
    println!();
    println!("tools: tree read search edit run check git expand batch sh");
    println!("utility: gain discover config handles parser");
    println!();
    println!("examples:");
    println!("  w tree src --depth 2");
    println!("  w read src/main.rs --lines 1:40");
    println!("  w search parse_config src");
    println!("  w run -- cargo test");
    println!("  w git diff --staged");
    println!("  w expand ~r7 --grep error");
}

// ---------------------------------------------------------------------------
// MCP entry
// ---------------------------------------------------------------------------

pub fn run_mcp(argv: Vec<String>) -> i32 {
    let (global, tool, rest) = match parse_global(&argv) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e.render());
            return e.cli_exit();
        }
    };
    if tool.is_some() || !rest.is_empty() {
        eprintln!("!invalid_args w-bash takes only global options");
        return 2;
    }
    match wbash_adapters::serve_stdio(
        &global.root,
        global.config.as_deref(),
        global.no_project_config,
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e.render());
            e.cli_exit()
        }
    }
}

pub fn util_map(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    config::counts_map(pairs)
}
