use crate::signal::{Events, TimeSeries};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Simple WFDB annotation entry.
#[derive(Debug, Clone)]
pub struct WfdbAnnotation {
    pub sample: usize,
    pub code: u8,
}

impl WfdbAnnotation {
    pub fn is_beat(&self) -> bool {
        self.code > 0 && self.code < 59
    }
}

/// Load the specified signal (lead) from a WFDB header/data pair into a TimeSeries.
pub fn load_wfdb_lead(header_path: &Path, lead: usize) -> Result<TimeSeries> {
    let (header, signals) = wfdb_rust::parse_wfdb(header_path);
    if lead >= signals.len() {
        anyhow::bail!(
            "WFDB record contains {} signals, but lead {} was requested",
            signals.len(),
            lead
        );
    }
    let spec = &header.signal_specs[lead];
    let raw = &signals[lead];
    let gain = spec.adc_gain.unwrap_or(1.0) as f64;
    let baseline = spec.baseline.or(spec.adc_zero).unwrap_or(0) as f64;
    let fs = header
        .record
        .sampling_frequency
        .map(|f| f as f64)
        .unwrap_or(250.0);
    let data = raw
        .iter()
        .map(|&sample| (sample as f64 - baseline) / gain)
        .collect();
    Ok(TimeSeries { fs, data })
}

/// Parse MIT annotation binary stream into samples & codes.
pub fn parse_wfdb_annotations(buf: &[u8]) -> Vec<WfdbAnnotation> {
    let mut out = Vec::new();
    let mut idx = 0;
    let mut sample: usize = 0;
    while idx + 2 <= buf.len() {
        let word = u16::from_le_bytes([buf[idx], buf[idx + 1]]);
        idx += 2;
        let code = (word >> 10) as u8;
        let diff = (word & 0x03FF) as usize;
        if code == 0 && diff == 0 {
            break;
        }
        match code {
            59 => {
                if idx + 4 > buf.len() {
                    break;
                }
                let high = u16::from_le_bytes([buf[idx], buf[idx + 1]]) as u32;
                let low = u16::from_le_bytes([buf[idx + 2], buf[idx + 3]]) as u32;
                idx += 4;
                let skip = (high << 16) | low;
                sample = sample.wrapping_add(skip as usize);
            }
            60..=62 => {
                // NUM/SUB/CHN: just ignore the payload
                sample = sample.wrapping_add(diff);
            }
            63 => {
                idx += diff as usize;
                if diff % 2 != 0 && idx < buf.len() {
                    idx += 1;
                }
            }
            _ => {
                sample = sample.wrapping_add(diff);
                out.push(WfdbAnnotation { sample, code });
            }
        }
    }
    out
}

fn read_exact(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("failed to read {}", path.display()))
}

/// Read WFDB annotation file (ATR) and convert to beat events.
pub fn load_wfdb_events(path: &Path) -> Result<Events> {
    let buf = read_exact(path)?;
    let beat_samples: Vec<usize> = parse_wfdb_annotations(&buf)
        .into_iter()
        .filter(WfdbAnnotation::is_beat)
        .map(|ann| ann.sample)
        .collect();
    Ok(Events::from_indices(beat_samples))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_simple_annotation_stream() {
        let mut bytes = vec![];
        // first annotation: code 1, diff=5 -> sample=5
        bytes.extend(&((1u16 << 10) | 5u16).to_le_bytes());
        // second annotation: code 2, diff=10 -> sample=15
        bytes.extend(&((2u16 << 10) | 10u16).to_le_bytes());
        // add SKIP to jump 5000 samples
        bytes.extend(&(59u16 << 10).to_le_bytes());
        bytes.extend(&0x0000u16.to_le_bytes());
        bytes.extend(&0x1388u16.to_le_bytes());
        // terminate
        bytes.extend(&0u16.to_le_bytes());

        let annotations = parse_wfdb_annotations(&bytes);
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].sample, 5);
        assert_eq!(annotations[1].sample, 15);
    }

    #[test]
    fn reads_mitdb_record() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        let header = root.join("test_data/mitdb/100.hea");
        let atr = root.join("test_data/mitdb/100.atr");
        let ts = load_wfdb_lead(&header, 0).expect("load sample lead");
        assert!(ts.fs > 300.0 && ts.fs < 400.0);
        assert!(ts.len() > 1000);
        let events = load_wfdb_events(&atr).expect("load annotations");
        assert!(!events.indices.is_empty());
    }
}
