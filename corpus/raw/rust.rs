use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub root: PathBuf,
    pub ignore: Vec<String>,
    pub max_depth: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { root: PathBuf::from("."), ignore: Vec::new(), max_depth: 32 }
    }
}

pub fn resolve(root: &Path, input: &str) -> Result<PathBuf, String> {
    let candidate = root.join(input);
    let canonical = candidate.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(root) {
        return Err(format!("outside workspace: {}", canonical.display()));
    }
    Ok(canonical)
}

pub fn summarize(entries: &[(String, usize)]) -> HashMap<&'static str, usize> {
    let mut out = HashMap::new();
    out.insert("files", entries.len());
    out.insert("bytes", entries.iter().map(|(_, n)| *n).sum());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inside_root() {
        assert!(resolve(Path::new("."), "src").is_ok());
    }
}
