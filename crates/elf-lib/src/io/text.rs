use anyhow::{Context, Result};
use std::path::Path;

/// Parse newline-delimited floating point series, ignoring blank/comment lines.
pub fn parse_f64_series(text: &str) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let val: f64 = trimmed
            .parse()
            .with_context(|| format!("line {} is not f64: {}", idx + 1, trimmed))?;
        out.push(val);
    }
    if out.is_empty() {
        anyhow::bail!("no numeric samples found");
    }
    Ok(out)
}

/// Read a newline-delimited floating point series from disk.
pub fn read_f64_series(path: &Path) -> Result<Vec<f64>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_f64_series(&text)
}

/// Parse newline-delimited sample indices (usize) into an Events-friendly list.
pub fn parse_event_indices(text: &str) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let val: usize = trimmed
            .parse()
            .with_context(|| format!("line {} is not an integer index: {}", idx + 1, trimmed))?;
        out.push(val);
    }
    if out.is_empty() {
        anyhow::bail!("no annotation indices found");
    }
    Ok(out)
}

/// Read event indices from a file.
pub fn read_event_indices(path: &Path) -> Result<Vec<usize>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    parse_event_indices(&text)
}
