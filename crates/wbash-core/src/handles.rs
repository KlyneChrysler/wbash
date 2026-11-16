use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Result, WErr};
use crate::hash;
use crate::paths;
use crate::redact::Redactor;
use crate::types::{now_ms, HandleRecord, HandleRef};

/// Session-scoped, content-addressed-ish blob store (spec §3.3, §11.2).
///
/// IDs are monotonically allocated base36 counters and are never reused within
/// a session. Blobs live under the private cache directory (0700/0600).
pub struct HandleStore {
    dir: PathBuf,
    manifest: PathBuf,
    handles: BTreeMap<String, HandleRecord>,
    next: u64,
    ttl_secs: u64,
    max_bytes: u64,
    session_hash: String,
    redactor: Redactor,
}

impl HandleStore {
    pub fn open(
        session_id: &str,
        root: &Path,
        ttl_secs: u64,
        max_bytes: u64,
        redactor: Redactor,
    ) -> Result<Self> {
        let session_hash = {
            let material = format!("{session_id}\u{0}{}", root.to_string_lossy());
            hash::hash_str(&material)[..16].to_string()
        };
        let base = paths::cache_dir().join("handles").join(&session_hash);
        paths::ensure_private_dir(&base)
            .map_err(|e| WErr::new("permission_denied", format!("handle dir: {e}")))?;
        let manifest = base.join("manifest.jsonl");
        let mut handles = BTreeMap::new();
        let mut next = 1u64;
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            for line in text.lines() {
                if let Ok(rec) = serde_json::from_str::<HandleRecord>(line) {
                    next = next.max(parse_id(&rec.id).map(|n| n + 1).unwrap_or(1));
                    handles.insert(rec.id.clone(), rec);
                }
            }
        }
        Ok(Self {
            dir: base,
            manifest,
            handles,
            next,
            ttl_secs,
            max_bytes,
            session_hash,
            redactor,
        })
    }

    /// Store text (redacted) or bytes and return a handle ref.
    pub fn put(&mut self, kind: &str, bytes: &[u8], content_type: &str) -> Result<HandleRef> {
        let (stored, redacted, ctype) = if content_type == "text" {
            let text = String::from_utf8_lossy(bytes);
            let (red, n) = self.redactor.redact(&text);
            (red.into_bytes(), n > 0, "text".to_string())
        } else {
            (bytes.to_vec(), false, "bytes".to_string())
        };
        let id = format!("~{}", base36(self.next));
        self.next += 1;
        let full = hash::hash_bytes(&stored);
        let now = now_ms();
        let rec = HandleRecord {
            id: id.clone(),
            kind: kind.to_string(),
            bytes: stored.len() as u64,
            hash: full.clone(),
            created_ms: now,
            expires_ms: now + (self.ttl_secs as i64) * 1000,
            last_access_ms: now,
            redacted,
            content_type: ctype,
        };
        let blob = self
            .dir
            .join(format!("{}.blob", id.trim_start_matches('~')));
        write_private(&blob, &stored)?;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.manifest)
        {
            if let Ok(line) = serde_json::to_string(&rec) {
                let _ = writeln!(f, "{line}");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
            }
        }
        let out = HandleRef {
            id: id.clone(),
            kind: rec.kind.clone(),
            bytes: rec.bytes,
            hash: rec.hash.clone(),
        };
        self.handles.insert(id, rec);
        self.evict_if_needed();
        Ok(out)
    }

    pub fn get(&mut self, id: &str) -> Result<(Vec<u8>, HandleRecord)> {
        let now = now_ms();
        let rec = self.handles.get(id).cloned().ok_or_else(|| {
            WErr::with_hint(
                "handle_not_found",
                format!("no handle '{id}' in this session"),
                "re-run the tool that produced it",
            )
        })?;
        if now > rec.expires_ms {
            self.remove(id);
            return Err(WErr::with_hint(
                "handle_expired",
                format!("handle '{id}' expired"),
                "re-run the tool that produced it",
            ));
        }
        let blob = self
            .dir
            .join(format!("{}.blob", id.trim_start_matches('~')));
        let bytes = std::fs::read(&blob).map_err(|_| {
            WErr::with_hint(
                "handle_expired",
                format!("handle '{id}' payload is gone"),
                "re-run the tool that produced it",
            )
        })?;
        if let Some(r) = self.handles.get_mut(id) {
            r.last_access_ms = now;
        }
        let rec = self.handles.get(id).cloned().unwrap();
        Ok((bytes, rec))
    }

    pub fn session_hash(&self) -> &str {
        &self.session_hash
    }

    pub fn total_bytes(&self) -> u64 {
        self.handles.values().map(|h| h.bytes).sum()
    }

    pub fn gc(&mut self) -> u64 {
        let now = now_ms();
        let expired: Vec<String> = self
            .handles
            .values()
            .filter(|h| now > h.expires_ms)
            .map(|h| h.id.clone())
            .collect();
        let mut removed = expired.len() as u64;
        for id in expired {
            self.remove(&id);
        }
        while self.total_bytes() > self.max_bytes {
            let victim = self
                .handles
                .values()
                .min_by_key(|h| h.last_access_ms)
                .map(|h| h.id.clone());
            match victim {
                Some(id) => {
                    self.remove(&id);
                    removed += 1;
                }
                None => break,
            }
        }
        self.rewrite_manifest();
        removed
    }

    fn evict_if_needed(&mut self) {
        if self.total_bytes() > self.max_bytes {
            self.gc();
        }
    }

    fn remove(&mut self, id: &str) {
        self.handles.remove(id);
        let blob = self
            .dir
            .join(format!("{}.blob", id.trim_start_matches('~')));
        let _ = std::fs::remove_file(blob);
    }

    fn rewrite_manifest(&self) {
        let mut text = String::new();
        for rec in self.handles.values() {
            if let Ok(line) = serde_json::to_string(rec) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        let _ = write_private(&self.manifest, text.as_bytes());
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = std::fs::File::create(path).map_err(|e| {
        WErr::new(
            "permission_denied",
            format!("write {}: {e}", path.display()),
        )
    })?;
    f.write_all(bytes)
        .map_err(|e| WErr::new("internal", format!("write {}: {e}", path.display())))?;
    f.sync_all().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "0".to_string())
}

fn parse_id(id: &str) -> Option<u64> {
    let s = id.trim_start_matches('~');
    u64::from_str_radix(s, 36).ok()
}
