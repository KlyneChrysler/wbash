use serde::Deserialize;
use serde_json::{json, Value};

use wbash_core::args::parse_args;
use wbash_core::error::{Result, WErr};
use wbash_core::formatter::{self, RenderOptions};
use wbash_core::registry::{invoke_tool, precheck, Tool, ToolContext};
use wbash_core::types::{Format, HandleRef, Item, ItemClass, Tier, ToolResult};

pub struct BatchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    calls: Vec<BatchCall>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchCall {
    tool: String,
    #[serde(default = "empty_obj")]
    args: Value,
}

fn empty_obj() -> Value {
    json!({})
}

impl Tool for BatchTool {
    fn name(&self) -> &'static str {
        "batch"
    }
    fn description(&self) -> &'static str {
        "Run up to 16 independent tool calls; child failures do not cancel."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {"type": "string"},
                            "args": {"type": "object"}
                        },
                        "required": ["tool"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["calls"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: BatchArgs = parse_args(
            args.clone(),
            "batch",
            "calls=[{\"tool\":\"read\",\"args\":{...}}]",
        )?;
        if a.calls.is_empty() {
            return Err(WErr::invalid_args(
                "calls must not be empty",
                "calls=[{tool,args}]",
            ));
        }
        let max = ctx.config.batch.max_calls;
        if a.calls.len() > max {
            return Err(WErr::invalid_args(
                format!("too many calls ({}, max {max})", a.calls.len()),
                format!("split into batches of at most {max}"),
            ));
        }
        for (i, call) in a.calls.iter().enumerate() {
            if call.tool == "batch" {
                return Err(WErr::invalid_args(
                    "nested batch is not supported",
                    format!("call #{i} is batch"),
                ));
            }
            precheck(ctx.registry, &call.tool, &call.args, ctx)?;
        }

        let n = a.calls.len();
        let per_child = (ctx.budget.saturating_sub(40) / n as u32).max(64);
        let mut result = ToolResult::new("batch", String::new());
        let mut ok_count = 0usize;
        let mut err_count = 0usize;

        for (i, call) in a.calls.iter().enumerate() {
            match invoke_tool(ctx.registry, &call.tool, &call.args, ctx) {
                Ok(child) => {
                    ok_count += 1;
                    result.push(Item::new(
                        ItemClass::High,
                        5.0,
                        vec![format!("#{} {} ok", i + 1, call.tool)],
                    ));
                    let rendered = formatter::render(
                        &child,
                        RenderOptions {
                            budget: per_child,
                            format: Format::Lean,
                            handles: ctx.handles,
                            redactor: ctx.redactor,
                            passthrough_tokens: Some(ctx.config.output.passthrough_tokens),
                            min_gain_pct: Some(ctx.config.output.min_gain_pct),
                            verbosity: Some(ctx.config.output.verbosity.clone()),
                        },
                    )?;
                    for line in rendered.text.lines() {
                        result.low(line.to_string());
                    }
                    for h in &child.handles {
                        if !result.handles.iter().any(|x| x.id == h.id) {
                            result.handle(HandleRef {
                                id: h.id.clone(),
                                kind: h.kind.clone(),
                                bytes: h.bytes,
                                hash: h.hash.clone(),
                            });
                        }
                    }
                }
                Err(e) => {
                    err_count += 1;
                    result.push(Item::new(
                        ItemClass::MustKeep,
                        1000.0,
                        vec![format!("#{} {} error", i + 1, call.tool)],
                    ));
                    result.must(e.render());
                }
            }
        }

        result.header = format!("{ok_count} ok, {err_count} error, {n} total");
        result.meta.ok = err_count == 0;
        result.meta.counts.insert("ok".into(), ok_count as i64);
        result.meta.counts.insert("error".into(), err_count as i64);
        result.meta.counts.insert("total".into(), n as i64);
        Ok(result)
    }
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(BatchTool));
}
