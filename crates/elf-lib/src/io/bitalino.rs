use anyhow::{Context, Result};
use csv::ReaderBuilder;
use std::path::Path;

use crate::signal::TimeSeries;

/// Load a BITalino/OpenSignals CSV and return a TimeSeries built from one analog column.
pub fn read_bitalino_csv(path: &Path, signal: &str) -> Result<TimeSeries> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let headers = reader.headers()?.clone();
    let timestamp_idx = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("timestamp"))
        .context("missing timestamp column")?;
    let column = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case(signal))
        .context(format!("missing signal column '{}'", signal))?;
    let mut values = Vec::new();
    let mut last_ts = None;
    let mut fs = 0.0;
    for record in reader.records() {
        let record = record.context("reading record")?;
        let timestamp_str = record
            .get(timestamp_idx)
            .ok_or_else(|| anyhow::anyhow!("missing timestamp column"))?;
        let ts: f64 = timestamp_str
            .parse()
            .with_context(|| format!("parsing timestamp {}", timestamp_str))?;
        let value_str = record
            .get(column)
            .ok_or_else(|| anyhow::anyhow!("missing signal column"))?;
        let value = value_str.parse::<f64>().context("parsing signal value")?;
        if let Some(prev) = last_ts {
            if fs == 0.0 {
                fs = 1.0 / (ts - prev);
            }
        }
        last_ts = Some(ts);
        values.push(value);
    }
    if fs <= 0.0 {
        fs = 1000.0;
    }
    Ok(TimeSeries { fs, data: values })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_bitalino_csv() {
        let path = sample_path("test_data/bitalino_sample.csv");
        let ts = read_bitalino_csv(&path, "analog0").expect("read sample");
        assert_eq!(ts.data.len(), 5);
        assert!(ts.fs > 0.0);
        assert!((ts.data[0] - 1.2).abs() < 1e-6);
    }

    fn sample_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join(relative)
    }
}
