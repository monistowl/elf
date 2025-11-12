use crate::signal::{Events, TimeSeries};
use anyhow::{anyhow, Context, Result};
use csv::{ReaderBuilder, StringRecord};
use edf_reader::file_reader::SyncFileReader;
use edf_reader::sync_reader::SyncEDFReader;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Helper implementing the EDF reader trait for on-disk files.
struct DiskFileReader {
    path: PathBuf,
}

impl DiskFileReader {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl SyncFileReader for DiskFileReader {
    fn read(&self, offset: u64, length: u64) -> Result<Vec<u8>, std::io::Error> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; length as usize];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }
}

/// Load a single EDF channel (by index) into a `TimeSeries`.
pub fn load_edf_channel(path: &Path, channel: usize) -> Result<TimeSeries> {
    let reader = SyncEDFReader::init_with_file_reader(DiskFileReader::new(path))?;
    if channel >= reader.edf_header.channels.len() {
        return Err(anyhow!(
            "EDF file has {} channels; channel {} is out of range",
            reader.edf_header.channels.len(),
            channel
        ));
    }
    let total_duration = reader.edf_header.block_duration * reader.edf_header.number_of_blocks;
    let data_matrix = reader.read_data_window(0, total_duration)?;
    let channel_data = data_matrix
        .get(channel)
        .ok_or_else(|| anyhow!("missing channel data"))?;
    let hdr_chan = &reader.edf_header.channels[channel];
    let fs = hdr_chan.number_of_samples_in_data_record as f64 * 1000.0
        / reader.edf_header.block_duration as f64;
    Ok(TimeSeries {
        fs,
        data: channel_data.iter().map(|value| *value as f64).collect(),
    })
}

/// Simple BIDS event descriptor extracted from an `events.tsv` file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BidsEvent {
    pub onset: f64,
    pub duration: Option<f64>,
    pub trial_type: Option<String>,
}

impl BidsEvent {
    fn from_record(
        record: &StringRecord,
        onset_idx: usize,
        duration_idx: Option<usize>,
        trial_idx: Option<usize>,
    ) -> Result<Self> {
        let onset = record
            .get(onset_idx)
            .ok_or_else(|| anyhow!("missing onset column"))?
            .parse::<f64>()
            .context("parsing onset")?;
        let duration = duration_idx
            .and_then(|idx| record.get(idx))
            .and_then(|value| value.parse::<f64>().ok());
        let trial_type = trial_idx
            .and_then(|idx| record.get(idx))
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.to_string());
        Ok(Self {
            onset,
            duration,
            trial_type,
        })
    }
}

/// Load BIDS events (`events.tsv`) and convert them to `Events` indices using the sampling rate.
pub fn load_bids_events_indices(path: &Path, fs: f64) -> Result<Events> {
    let events = load_bids_events(path)?;
    let indices = events
        .into_iter()
        .map(|event| (event.onset * fs).round() as usize)
        .collect();
    Ok(Events::from_indices(indices))
}

/// Load BIDS `events.tsv` into structured `BidsEvent` rows.
pub fn load_bids_events(path: &Path) -> Result<Vec<BidsEvent>> {
    let mut reader = ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let headers = reader.headers()?.clone();
    let onset_idx = headers
        .iter()
        .position(|header| header.eq_ignore_ascii_case("onset"))
        .ok_or_else(|| anyhow!("events.tsv must include an onset column"))?;
    let duration_idx = headers
        .iter()
        .position(|header| header.eq_ignore_ascii_case("duration"));
    let trial_idx = headers
        .iter()
        .position(|header| header.eq_ignore_ascii_case("trial_type"));
    let mut out = Vec::new();
    for result in reader.records() {
        let record = result.context("reading events record")?;
        out.push(BidsEvent::from_record(
            &record,
            onset_idx,
            duration_idx,
            trial_idx,
        )?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_bids_events_file() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("test_data");
        let path = dir.join("bids_sample.tsv");
        let events = load_bids_events(&path).expect("read sample events");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].onset, 0.0);
        assert_eq!(events[2].trial_type.as_deref(), Some("task"));
    }

    #[test]
    fn bids_indices_respect_sampling_rate() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest_dir.parent().unwrap().parent().unwrap();
        let events = load_bids_events(&dir.join("test_data/bids_sample.tsv")).unwrap();
        let indices =
            load_bids_events_indices(&dir.join("test_data/bids_sample.tsv"), 250.0).unwrap();
        assert_eq!(indices.indices.len(), events.len());
        assert_eq!(indices.indices[1], 125);
    }
}
