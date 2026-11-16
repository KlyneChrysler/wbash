use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use wbash_index::{SymbolIndex, Workspace};

use crate::args::{default_budget, strip_common};
use crate::config::Config;
use crate::error::{Result, WErr};
use crate::formatter::{self, RenderOptions, Rendered};
use crate::handles::HandleStore;
use crate::hash;
use crate::parsers::{NoParsers, ParserSet};
use crate::paths;
use crate::policy;
use crate::redact::Redactor;
use crate::registry::{invoke_tool, Registry, ToolContext};
use crate::stats;
use crate::types::{now_ms, Format, Ledger, Tier};

pub struct SessionState {
    pub tier: Tier,
    pub ledger: Ledger,
    pub handles: HandleStore,
    pub started_ms: i64,
    pub last_seen_ms: i64,
}

pub struct Dispatcher {
    config: Config,
    root: PathBuf,
    registry: Arc<Registry>,
    workspace: Workspace,
    symbols: SymbolIndex,
    parsers: Arc<dyn ParserSet>,
    redactor: Redactor,
    sessions: HashMap<String, SessionState>,
    allowlist: Vec<PathBuf>,
}

impl Dispatcher {
    pub fn new(
        config: Config,
        root: &Path,
        registry: Arc<Registry>,
        parsers: Arc<dyn ParserSet>,
    ) -> Result<Self> {
        let root = policy::canonical_root(root)?;
        let allowlist = config.allowlist_paths();
        let workspace =
            Workspace::new(root.clone()).with_max_entries(config.index.max_tree_entries);
        Ok(Self {
            config,
            root,
            registry,
            workspace,
            symbols: SymbolIndex::new(),
            parsers,
            redactor: Redactor::new(),
            sessions: HashMap::new(),
            allowlist,
        })
    }

    pub fn with_no_parsers(config: Config, root: &Path, registry: Arc<Registry>) -> Result<Self> {
        Self::new(config, root, registry, Arc::new(NoParsers))
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn symbols(&self) -> &SymbolIndex {
        &self.symbols
    }

    pub fn redactor(&self) -> &Redactor {
        &self.redactor
    }

    pub fn parsers(&self) -> &dyn ParserSet {
        self.parsers.as_ref()
    }

    pub fn open_session(&mut self, session_id: &str) -> Result<()> {
        self.ensure_session(session_id).map(|_| ())
    }

    pub fn set_session_tier(&mut self, session_id: &str, tier: Tier) -> Result<()> {
        self.ensure_session(session_id)?.tier = tier;
        Ok(())
    }

    pub fn call(&mut self, tool: &str, args: &Value, session_id: &str) -> Rendered {
        let format = peek_format(args);
        match self.call_inner(tool, args, session_id) {
            Ok(r) => r,
            Err(e) => {
                stats::audit(&self.config, tool, false, Some(e.code));
                Rendered::error(&e, format)
            }
        }
    }

    fn call_inner(&mut self, tool: &str, args: &Value, session_id: &str) -> Result<Rendered> {
        let session_id = if session_id.is_empty() {
            "default"
        } else {
            session_id
        };
        let (common, tool_args) = strip_common(args.clone(), tool)?;
        self.ensure_session(session_id)?;
        let budget = common.budget.unwrap_or_else(|| default_budget(tool));
        let key = session_id.to_string();

        let result = {
            let session = self.sessions.get_mut(&key).expect("session");
            let mut ctx = ToolContext {
                root: &self.root,
                registry: &self.registry,
                tier: session.tier,
                config: &self.config,
                workspace: &self.workspace,
                symbols: &self.symbols,
                parsers: self.parsers.as_ref(),
                handles: &mut session.handles,
                ledger: &mut session.ledger,
                redactor: &self.redactor,
                session_id,
                budget,
                allowlist: &self.allowlist,
            };
            invoke_tool(&self.registry, tool, &tool_args, &mut ctx)
        };

        let result = match result {
            Ok(r) => r,
            Err(e) => {
                stats::audit(&self.config, tool, false, Some(e.code));
                return Err(e);
            }
        };

        let rendered = {
            let session = self.sessions.get_mut(&key).expect("session");
            formatter::render(
                &result,
                RenderOptions {
                    budget,
                    format: common.format,
                    handles: &mut session.handles,
                    redactor: &self.redactor,
                },
            )?
        };

        stats::record_result(&self.config, &result, rendered.out_tokens);
        stats::audit(&self.config, tool, result.meta.ok, None);

        let saved = if let Some(session) = self.sessions.get_mut(&key) {
            session.last_seen_ms = now_ms();
            session
                .ledger
                .expire(self.config.session.ledger_ttl_secs, now_ms());
            Some(session.ledger.clone())
        } else {
            None
        };
        if let Some(ledger) = saved {
            self.save_ledger(session_id, &ledger);
        }
        self.prune_sessions();
        Ok(rendered)
    }

    fn ensure_session(&mut self, session_id: &str) -> Result<&mut SessionState> {
        if !self.sessions.contains_key(session_id) {
            let tier: Tier = self.config.session.default_tier.parse()?;
            let handles = HandleStore::open(
                session_id,
                &self.root,
                self.config.handles.ttl_secs,
                self.config.handles.max_bytes,
                Redactor::new(),
            )?;
            let now = now_ms();
            let ledger = self.load_ledger(session_id);
            self.sessions.insert(
                session_id.to_string(),
                SessionState {
                    tier,
                    ledger,
                    handles,
                    started_ms: now,
                    last_seen_ms: now,
                },
            );
        }
        self.sessions
            .get_mut(session_id)
            .ok_or_else(|| WErr::internal("session disappeared"))
    }

    fn ledger_file(&self, session_id: &str) -> std::path::PathBuf {
        let material = format!("{session_id}\u{0}{}", self.root.to_string_lossy());
        let name = format!("{}.json", &hash::hash_str(&material)[..16]);
        paths::cache_dir().join("ledger").join(name)
    }

    fn load_ledger(&self, session_id: &str) -> Ledger {
        let path = self.ledger_file(session_id);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ledger::default();
        };
        let mut ledger: Ledger = serde_json::from_str(&text).unwrap_or_default();
        ledger.expire(self.config.session.ledger_ttl_secs, now_ms());
        ledger
    }

    fn save_ledger(&self, session_id: &str, ledger: &Ledger) {
        let path = self.ledger_file(session_id);
        if let Some(parent) = path.parent() {
            if paths::ensure_private_dir(parent).is_err() {
                return;
            }
        }
        if let Ok(text) = serde_json::to_vec(ledger) {
            let _ = std::fs::write(&path, text);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    fn prune_sessions(&mut self) {
        if self.sessions.len() <= 32 {
            return;
        }
        let now = now_ms();
        let ttl = (self.config.session.ledger_ttl_secs.max(7200) as i64) * 1000;
        self.sessions.retain(|_, s| now - s.last_seen_ms <= ttl);
    }

    pub fn gc_handles(&mut self, session_id: &str) -> Result<u64> {
        let session = self.ensure_session(session_id)?;
        Ok(session.handles.gc())
    }
}

fn peek_format(args: &Value) -> Format {
    args.get("format")
        .and_then(Value::as_str)
        .and_then(|s| Format::parse(s).ok())
        .unwrap_or(Format::Lean)
}
