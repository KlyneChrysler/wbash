//! Config parsing.
pub fn parse_config(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("empty".to_string());
    }
    Ok(path.to_string())
}
