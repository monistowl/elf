use anyhow::{Context, Result};
use csv::ReaderBuilder;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Eye {
    Left,
    Right,
    Binocular,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PupilSample {
    pub timestamp: f64,
    pub pupil_mm: Option<f32>,
    pub confidence: Option<f32>,
    pub eye: Eye,
}

pub fn read_eye_csv(
    path: &Path,
    timestamp_col: &str,
    pupil_col: &str,
    confidence_col: Option<&str>,
    eye_label_col: Option<&str>,
    delimiter: u8,
) -> Result<Vec<PupilSample>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut reader = ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .from_reader(file);
    let headers = reader.headers().context("reading header")?.clone();

    let ts_idx = locate_column(&headers, timestamp_col, "timestamp")?;
    let pupil_idx = locate_column(&headers, pupil_col, "pupil diameter")?;
    let conf_idx = confidence_col.and_then(|col| locate_column(&headers, col, "confidence").ok());
    let eye_idx = eye_label_col.and_then(|col| locate_column(&headers, col, "eye label").ok());

    let mut samples = Vec::new();
    for result in reader.records() {
        let record = result.context("reading record")?;
        let timestamp = record
            .get(ts_idx)
            .ok_or_else(|| anyhow::anyhow!("missing timestamp"))?
            .parse::<f64>()
            .context("parsing timestamp")?;
        let pupil = record.get(pupil_idx).and_then(|v| v.parse::<f32>().ok());
        let confidence = conf_idx
            .and_then(|idx| record.get(idx))
            .and_then(|v| v.parse::<f32>().ok());
        let eye = eye_idx
            .and_then(|idx| record.get(idx))
            .map(|value| match value.to_lowercase().as_str() {
                "l" | "left" => Eye::Left,
                "r" | "right" => Eye::Right,
                _ => Eye::Binocular,
            })
            .unwrap_or(Eye::Binocular);
        samples.push(PupilSample {
            timestamp,
            pupil_mm: pupil,
            confidence,
            eye,
        });
    }
    Ok(samples)
}

pub fn confidence_filter(samples: &[PupilSample], min_confidence: f32) -> Vec<PupilSample> {
    samples
        .iter()
        .filter(|sample| sample.confidence.unwrap_or(1.0) >= min_confidence)
        .cloned()
        .collect()
}

fn locate_column(headers: &csv::StringRecord, requested: &str, hint: &str) -> Result<usize> {
    headers
        .iter()
        .position(|name| name.eq_ignore_ascii_case(requested))
        .ok_or_else(|| anyhow::anyhow!("missing {} column ({})", hint, requested))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn reads_pupil_csv() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace")
            .join("test_data/eye_sample.csv");
        let samples = read_eye_csv(
            &path,
            "timestamp",
            "pupil_left",
            Some("confidence"),
            Some("eye"),
            b',',
        )
        .unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].eye, Eye::Left);
        assert!(samples[1].pupil_mm.is_none());
    }

    #[test]
    fn reads_pupil_labs_export() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace")
            .join("test_data/pupil_labs_sample.csv");
        let samples = read_eye_csv(
            &path,
            "timestamp",
            "diameter",
            Some("confidence"),
            Some("eye"),
            b',',
        )
        .unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[1].eye, Eye::Right);
    }

    #[test]
    fn reads_tobii_export() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace")
            .join("test_data/tobii_sample.tsv");
        let samples = read_eye_csv(
            &path,
            "system_time_stamp",
            "pupil_diameter_2d",
            Some("confidence"),
            Some("eye"),
            b'\t',
        )
        .unwrap();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].pupil_mm, Some(4.12));
    }

    #[test]
    fn filters_confidence() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = manifest_dir.parent().and_then(|p| p.parent()).unwrap();
        let path = dir.join("test_data/eye_sample.csv");
        let samples = read_eye_csv(
            &path,
            "timestamp",
            "pupil_left",
            Some("confidence"),
            Some("eye"),
            b',',
        )
        .unwrap();
        let filtered = confidence_filter(&samples, 0.9);
        assert_eq!(filtered.len(), 2);
    }
}
