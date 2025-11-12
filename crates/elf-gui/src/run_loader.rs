use anyhow::{Context, Result};
use csv::{ReaderBuilder, Trim};
use elf_lib::signal::Events;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct RunEventRecord {
    onset: f64,
    event_type: String,
}

pub fn load_manifest(path: &Path) -> Result<RunManifest> {
    let file =
        fs::File::open(path).with_context(|| format!("reading manifest {}", path.display()))?;
    let manifest: RunManifest = serde_json::from_reader(file)
        .with_context(|| format!("parsing manifest {}", path.display()))?;
    Ok(manifest)
}

pub fn load_events(path: &Path) -> Result<Vec<f64>> {
    let mut reader = ReaderBuilder::new()
        .delimiter(b'\t')
        .trim(Trim::All)
        .from_path(path)
        .with_context(|| format!("reading events {}", path.display()))?;
    let mut times = Vec::new();
    for result in reader.deserialize::<RunEventRecord>() {
        let record = result?;
        if record.event_type == "stim" {
            times.push(record.onset);
        }
    }
    Ok(times)
}

pub fn events_from_times(times: &[f64], fs: f64) -> Events {
    let indices = times
        .iter()
        .map(|&t| ((t.max(0.0)) * fs).round() as usize)
        .collect();
    Events::from_indices(indices)
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
        let times = load_events(&path).unwrap();
        assert_eq!(times.len(), 1);
    }
}
