use anyhow::{Context, Result};
use csv::{ReaderBuilder, Trim};
use elf_lib::signal::Events;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunManifest {
    pub task: String,
    pub design: String,
    pub total_trials: usize,
    pub total_events: usize,
    pub seed: Option<u64>,
    pub randomization_policy: Option<String>,
    pub isi_ms: f64,
    pub isi_jitter_ms: Option<f64>,
    pub start_time_unix: f64,
}

/// Flexible filter when loading run bundle events.
#[derive(Debug, Clone, Serialize)]
pub struct RunEventFilter {
    pub onset_column: String,
    pub event_type_column: String,
    pub duration_column: Option<String>,
    pub label_column: Option<String>,
    pub allowed_event_types: Vec<String>,
}

impl Default for RunEventFilter {
    fn default() -> Self {
        Self {
            onset_column: "onset".into(),
            event_type_column: "event_type".into(),
            duration_column: Some("duration".into()),
            label_column: Some("event_type".into()),
            allowed_event_types: vec!["stim".into()],
        }
    }
}

impl RunEventFilter {
    pub fn allow_all() -> Self {
        let mut filter = Self::default();
        filter.allowed_event_types.clear();
        filter
    }

    pub fn with_allowed_types(types: Vec<String>) -> Self {
        let mut filter = Self::default();
        filter.allowed_event_types = types;
        filter
    }

    fn matches(&self, value: &str) -> bool {
        let normalized = value.trim().to_ascii_lowercase();
        if self.allowed_event_types.is_empty() {
            return true;
        }
        self.allowed_event_types
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&normalized))
    }
}

/// Event extracted from the run bundle TSV.
#[derive(Debug, Clone)]
pub struct RunEventRecord {
    pub onset: f64,
    pub duration: Option<f64>,
    pub event_type: String,
    pub label: Option<String>,
}

pub fn load_manifest(path: &Path) -> Result<RunManifest> {
    let file =
        fs::File::open(path).with_context(|| format!("reading manifest {}", path.display()))?;
    let manifest: RunManifest = serde_json::from_reader(file)
        .with_context(|| format!("parsing manifest {}", path.display()))?;
    Ok(manifest)
}

pub fn load_events(path: &Path) -> Result<Vec<RunEventRecord>> {
    load_events_with_filter(path, &RunEventFilter::default())
}

pub fn load_events_with_filter(
    path: &Path,
    filter: &RunEventFilter,
) -> Result<Vec<RunEventRecord>> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b'\t')
        .trim(Trim::All)
        .from_path(path)
        .with_context(|| format!("reading events {}", path.display()))?;
    let headers = reader.headers()?.clone();
    let header_idxs: HashMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(idx, h)| (h.to_ascii_lowercase(), idx))
        .collect();
    let onset_idx = header_idx(&header_idxs, filter.onset_column.as_str()).unwrap_or(0);
    let type_idx =
        header_idx(&header_idxs, filter.event_type_column.as_str()).unwrap_or(onset_idx + 1);
    let duration_idx = filter
        .duration_column
        .as_deref()
        .and_then(|name| header_idx(&header_idxs, name));
    let label_idx = filter
        .label_column
        .as_deref()
        .and_then(|name| header_idx(&header_idxs, name));
    let mut records = Vec::new();
    for result in reader.records() {
        let record = result?;
        let event_type = record.get(type_idx).unwrap_or("");
        if !filter.matches(event_type) {
            continue;
        }
        let onset_str = record.get(onset_idx).unwrap_or("");
        let onset = onset_str
            .parse::<f64>()
            .with_context(|| format!("parsing onset {}", onset_str))?;
        let duration = duration_idx
            .and_then(|idx| record.get(idx))
            .and_then(|value| value.parse::<f64>().ok());
        let label = label_idx
            .and_then(|idx| record.get(idx))
            .map(|v| v.to_string());
        records.push(RunEventRecord {
            onset,
            duration,
            event_type: event_type.to_string(),
            label,
        });
    }
    Ok(records)
}

pub fn events_from_times(times: &[f64], fs: f64) -> Events {
    let indices = times
        .iter()
        .map(|&t| ((t.max(0.0)) * fs).round() as usize)
        .collect();
    Events::from_indices(indices)
}

pub fn events_from_records(records: &[RunEventRecord], fs: f64) -> Events {
    let times: Vec<f64> = records.iter().map(|record| record.onset).collect();
    events_from_times(&times, fs)
}

fn header_idx(headers: &HashMap<String, usize>, name: &str) -> Option<usize> {
    headers.get(&name.to_ascii_lowercase()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv::WriterBuilder;
    use tempfile::tempdir;

    #[test]
    fn loads_manifest_json() {
        let dir = tempdir().unwrap();
        let manifest = RunManifest {
            task: "test".into(),
            design: "test".into(),
            total_trials: 1,
            total_events: 2,
            seed: Some(1),
            randomization_policy: Some("permute".into()),
            isi_ms: 500.0,
            isi_jitter_ms: Some(100.0),
            start_time_unix: 0.0,
        };
        let path = dir.path().join("run.json");
        let file = fs::File::create(&path).unwrap();
        serde_json::to_writer(file, &manifest).unwrap();
        let parsed = load_manifest(&path).unwrap();
        assert_eq!(parsed.task, "test");
    }

    #[test]
    fn loads_events_tsv() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.tsv");
        let mut writer = WriterBuilder::new()
            .delimiter(b'\t')
            .from_path(&path)
            .unwrap();
        writer
            .write_record(&["onset", "duration", "event_type"])
            .unwrap();
        writer.write_record(&["0.0", "0.8", "stim"]).unwrap();
        writer.write_record(&["0.9", "0.0", "response"]).unwrap();
        writer.flush().unwrap();
        let records = load_events(&path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].event_type, "stim");
        assert_eq!(records[0].duration, Some(0.8));
    }

    #[test]
    fn apply_custom_filter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("events.tsv");
        let mut writer = WriterBuilder::new()
            .delimiter(b'\t')
            .from_path(&path)
            .unwrap();
        writer
            .write_record(&["Onset", "Event_Type", "Label"])
            .unwrap();
        writer.write_record(&["0.0", "stim", "S1"]).unwrap();
        writer.write_record(&["1.0", "response", "R1"]).unwrap();
        writer.write_record(&["2.0", "target", "T1"]).unwrap();
        writer.flush().unwrap();
        let filter = RunEventFilter::with_allowed_types(vec!["target".into(), "response".into()]);
        let records = load_events_with_filter(&path, &filter).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|rec| rec.event_type == "response"));
        assert!(records.iter().any(|rec| rec.event_type == "target"));
    }
}
