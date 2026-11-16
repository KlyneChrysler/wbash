src/config.rs (7 lines, b3:235dd1f276df68c1)
1: //! Config parsing.
2: pub fn parse_config(path: &str) -> Result<String, String> {
3:     if path.is_empty() {
4:         return Err("empty".to_string());
5:     }
6:     Ok(path.to_string())
7: }
