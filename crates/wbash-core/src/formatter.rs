use serde::Serialize;

use crate::error::{Result, WErr};
use crate::estimator;
use crate::handles::HandleStore;
use crate::redact::Redactor;
use crate::types::{Format, ItemClass, Meta, ToolResult};

pub struct RenderOptions<'a> {
    pub budget: u32,
    pub format: Format,
    pub handles: &'a mut HandleStore,
    pub redactor: &'a Redactor,
    pub passthrough_tokens: Option<u32>,
    pub min_gain_pct: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub text: String,
    pub ok: bool,
    pub cli_code: i32,
    pub is_error: bool,
    pub out_tokens: u32,
    pub redactions: u32,
}

impl Rendered {
    pub fn error(err: &WErr, format: Format) -> Self {
        let text = match format {
            Format::Lean => format!("{}\n", err.render()),
            Format::Json => render_error_json(err),
        };
        let out_tokens = estimator::estimate_text(&text);
        Self {
            text,
            ok: false,
            cli_code: err.cli_exit(),
            is_error: true,
            out_tokens,
            redactions: 0,
        }
    }
}

/// Lean wire-format rendering with value-ranked budget fitting (spec §6, §7.1).
pub fn render_lean(result: &ToolResult, opts: RenderOptions<'_>) -> Result<Rendered> {
    let mut redactions = 0u32;
    let (header, n) = opts.redactor.redact(&result.header);
    redactions += n;

    let must_tokens: u32 = result
        .items
        .iter()
        .filter(|i| i.class == ItemClass::MustKeep)
        .map(|i| i.est_tokens)
        .sum();
    let header_tokens = estimator::estimate_text(&header);
    let budget_exceeded = must_tokens + header_tokens > opts.budget;

    let mut remaining = opts
        .budget
        .saturating_sub(header_tokens)
        .saturating_sub(must_tokens);
    let mut included = vec![false; result.items.len()];

    // Greedy fill by value per token, stable by original order.
    let mut order: Vec<usize> = result
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.class != ItemClass::MustKeep)
        .map(|(idx, _)| idx)
        .collect();
    order.sort_by(|&a, &b| {
        let ia = &result.items[a];
        let ib = &result.items[b];
        let ra = ia.value / (ia.est_tokens.max(1) as f32);
        let rb = ib.value / (ib.est_tokens.max(1) as f32);
        rb.partial_cmp(&ra)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    for idx in order {
        let item = &result.items[idx];
        if item.est_tokens <= remaining {
            remaining -= item.est_tokens;
            included[idx] = true;
        }
    }

    let mut out: Vec<String> = Vec::new();
    out.push(header);
    for note in &result.meta.notes {
        let (line, n) = opts.redactor.redact(&format!("# {note}"));
        redactions += n;
        out.push(line);
    }
    if budget_exceeded {
        out.push("# budget_exceeded_must_keep".to_string());
    }

    let mut dropped_lines: Vec<String> = Vec::new();
    let emit_dropped = |out: &mut Vec<String>,
                        dropped: &mut Vec<String>,
                        handles: &mut HandleStore,
                        redactor: &Redactor,
                        redactions: &mut u32|
     -> Result<()> {
        if dropped.is_empty() {
            return Ok(());
        }
        let n = dropped.len();
        let joined = dropped.join("\n");
        let href = handles.put("elide", joined.as_bytes(), "text")?;
        let _ = redactor;
        out.push(format!("...+{n} lines ({})", href.id));
        *redactions += 0;
        dropped.clear();
        Ok(())
    };

    let mut body_from_items = false;
    for (idx, item) in result.items.iter().enumerate() {
        if included[idx] || item.class == ItemClass::MustKeep {
            emit_dropped(
                &mut out,
                &mut dropped_lines,
                opts.handles,
                opts.redactor,
                &mut redactions,
            )?;
            for line in &item.lines {
                let (red, n) = opts.redactor.redact(line);
                redactions += n;
                out.push(red);
            }
            body_from_items = true;
        } else {
            dropped_lines.extend(item.lines.iter().cloned());
        }
    }
    emit_dropped(
        &mut out,
        &mut dropped_lines,
        opts.handles,
        opts.redactor,
        &mut redactions,
    )?;

    // N1: passthrough for small/unstructured outputs when formatting isn't a clear win.
    if let (Some(raw), Some(pt), Some(min_gain)) = (
        result.raw_text.as_deref(),
        opts.passthrough_tokens,
        opts.min_gain_pct,
    ) {
        let raw_t = estimator::estimate_text(raw);
        let unstructured = result.diagnostics.is_empty()
            && result.handles.is_empty()
            && result
                .meta
                .counts
                .keys()
                .all(|k| matches!(k.as_str(), "raw_bytes" | "duration_ms"));
        if unstructured && raw_t <= pt {
            let mut pfx = vec![out[0].clone()];
            pfx.extend(
                out.iter()
                    .skip(1)
                    .take_while(|l| l.starts_with('#'))
                    .cloned(),
            );
            let (rraw, rn) = opts.redactor.redact(raw.trim_end());
            redactions += rn;
            let mut ptext = pfx.join("\n");
            ptext.push('\n');
            if !rraw.is_empty() {
                ptext.push_str(&rraw);
                ptext.push('\n');
            }
            let p_tokens = estimator::estimate_text(&ptext);
            let cur = estimator::estimate_text(&(out.join("\n") + "\n"));
            if !body_from_items
                || p_tokens
                    <= cur
                        .saturating_sub(cur.saturating_mul(min_gain) / 100)
                        .max(p_tokens)
                || raw_t <= pt / 2 + 1
            {
                return Ok(Rendered {
                    out_tokens: p_tokens,
                    text: ptext,
                    ok: result.meta.ok,
                    cli_code: if result.meta.ok { 0 } else { 1 },
                    is_error: false,
                    redactions,
                });
            }
        }
    }

    let mut text = out.join("\n");
    text.push('\n');

    Ok(Rendered {
        out_tokens: estimator::estimate_text(&text),
        text,
        ok: result.meta.ok,
        cli_code: if result.meta.ok { 0 } else { 1 },
        is_error: false,
        redactions,
    })
}

#[derive(Serialize)]
struct JsonError<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonOut<'a> {
    ok: bool,
    tool: &'a str,
    meta: &'a Meta,
    data: JsonData,
    diagnostics: &'a [crate::types::Diagnostic],
    handles: &'a [crate::types::HandleRef],
    errors: Vec<JsonError<'a>>,
}

#[derive(Serialize)]
struct JsonData {
    header: String,
    lines: Vec<String>,
    elided_lines: usize,
}

pub fn render_json(result: &ToolResult, opts: RenderOptions<'_>) -> Result<Rendered> {
    let mut redactions = 0u32;
    let (header, n) = opts.redactor.redact(&result.header);
    redactions += n;
    let mut lines: Vec<String> = Vec::new();
    let mut elided = 0usize;

    let must_tokens: u32 = result
        .items
        .iter()
        .filter(|i| i.class == ItemClass::MustKeep)
        .map(|i| i.est_tokens)
        .sum();
    let mut remaining = opts
        .budget
        .saturating_sub(estimator::estimate_text(&header) + must_tokens);

    let mut order: Vec<usize> = result
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.class != ItemClass::MustKeep)
        .map(|(idx, _)| idx)
        .collect();
    order.sort_by(|&a, &b| {
        let ia = &result.items[a];
        let ib = &result.items[b];
        let ra = ia.value / (ia.est_tokens.max(1) as f32);
        let rb = ib.value / (ib.est_tokens.max(1) as f32);
        rb.partial_cmp(&ra)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut included = vec![false; result.items.len()];
    for idx in order {
        let item = &result.items[idx];
        if item.est_tokens <= remaining {
            remaining -= item.est_tokens;
            included[idx] = true;
        }
    }
    for (idx, item) in result.items.iter().enumerate() {
        if included[idx] || item.class == ItemClass::MustKeep {
            for line in &item.lines {
                let (red, n) = opts.redactor.redact(line);
                redactions += n;
                lines.push(red);
            }
        } else {
            elided += item.lines.len();
        }
    }

    let mut meta = result.meta.clone();
    meta.redactions = meta.redactions.saturating_add(redactions);
    let out = JsonOut {
        ok: result.meta.ok,
        tool: result.tool,
        meta: &meta,
        data: JsonData {
            header,
            lines,
            elided_lines: elided,
        },
        diagnostics: &result.diagnostics,
        handles: &result.handles,
        errors: Vec::new(),
    };
    let text = serde_json::to_string(&out)? + "\n";
    Ok(Rendered {
        out_tokens: estimator::estimate_text(&text),
        text,
        ok: result.meta.ok,
        cli_code: if result.meta.ok { 0 } else { 1 },
        is_error: false,
        redactions,
    })
}

pub fn render(result: &ToolResult, opts: RenderOptions<'_>) -> Result<Rendered> {
    match opts.format {
        Format::Lean => render_lean(result, opts),
        Format::Json => render_json(result, opts),
    }
}

fn render_error_json(err: &WErr) -> String {
    let v = serde_json::json!({
        "ok": false,
        "tool": "",
        "meta": serde_json::Value::Null,
        "data": serde_json::Value::Null,
        "diagnostics": [],
        "handles": [],
        "errors": [{
            "code": err.code,
            "message": err.message,
            "hint": err.hint,
        }],
    });
    v.to_string() + "\n"
}
