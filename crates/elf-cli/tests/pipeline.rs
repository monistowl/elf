use assert_cmd::cargo::cargo_bin_cmd;
use elf_lib::metrics::hrv::HRVTime;
use elf_lib::signal::{Events, RRSeries};
use serde::Deserialize;
use std::{error::Error, fs, path::PathBuf};

#[derive(Deserialize)]
struct PipelineOutput {
    hrv: HRVTime,
}

#[derive(Deserialize)]
struct ExpectedFile {
    #[allow(dead_code)]
    fs: f64,
    #[allow(dead_code)]
    rr: Vec<f64>,
    hrv: HRVTime,
}

#[derive(Deserialize)]
struct MitdbPipelineResult {
    fs: f64,
    events: Events,
    rr: RRSeries,
    hrv: HRVTime,
}

#[test]
fn beat_pipeline_matches_expected_metrics() -> Result<(), Box<dyn Error>> {
    let test_data_dir = workspace_root().join("test_data");
    let recording = test_data_dir.join("synthetic_recording_a.txt");
    let expected_path = test_data_dir.join("synthetic_recording_a_expected.json");

    let expected: ExpectedFile = serde_json::from_str(&fs::read_to_string(expected_path)?)?;

    let mut cmd = cargo_bin_cmd!("elf");
    cmd.args([
        "beat-hrv-pipeline",
        "--fs",
        "250",
        "--input",
        recording.to_str().expect("utf8 path"),
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let actual: PipelineOutput = serde_json::from_slice(&output)?;

    assert_eq!(actual.hrv.n, expected.hrv.n);
    assert_close(actual.hrv.avnn, expected.hrv.avnn, 1e-3);
    assert_close(actual.hrv.sdnn, expected.hrv.sdnn, 3e-3);
    assert_close(actual.hrv.rmssd, expected.hrv.rmssd, 3e-3);
    assert_close(actual.hrv.pnn50, expected.hrv.pnn50, 1e-3);

    Ok(())
}

#[test]
fn beat_pipeline_handles_bids_events() -> Result<(), Box<dyn Error>> {
    let synthetic = sample_path("test_data/synthetic_recording_a.txt");
    let bids = sample_path("test_data/bids_sample.tsv");

    let mut cmd = cargo_bin_cmd!("elf");
    cmd.args([
        "beat-hrv-pipeline",
        "--fs",
        "250",
        "--input",
        &synthetic,
        "--bids-events",
        &bids,
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let actual: PipelineOutput = serde_json::from_slice(&output)?;

    assert_close(actual.hrv.avnn, 0.6, 1e-6);
    assert_close(actual.hrv.sdnn, 0.1414213562373095, 1e-6);
    assert_close(actual.hrv.rmssd, 0.2, 1e-6);
    assert_close(actual.hrv.pnn50, 1.0, 1e-6);
    Ok(())
}

#[test]
fn beat_pipeline_handles_mitdb_record() -> Result<(), Box<dyn Error>> {
    let test_data_dir = workspace_root().join("test_data");
    let header = test_data_dir.join("mitdb/100.hea");
    let annotations = test_data_dir.join("mitdb/100.atr");

    let mut cmd = cargo_bin_cmd!("elf");
    cmd.args([
        "beat-hrv-pipeline",
        "--wfdb-header",
        header.to_string_lossy().as_ref(),
        "--annotations",
        annotations.to_string_lossy().as_ref(),
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let actual: MitdbPipelineResult = serde_json::from_slice(&output)?;

    assert!(
        actual.fs > 300.0,
        "expected MIT-BIH fs around 360, got {}",
        actual.fs
    );
    assert!(!actual.events.indices.is_empty(), "should find beats");
    assert!(!actual.rr.rr.is_empty(), "should produce RR intervals");
    assert!(actual.hrv.n > 0, "HRV summary should have entries");
    Ok(())
}

fn assert_close(a: f64, b: f64, tol: f64) {
    let diff = (a - b).abs();
    assert!(
        diff <= tol,
        "diff {} exceeded tol {} ({} vs {})",
        diff,
        tol,
        a,
        b
    );
}

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn sample_path(relative: &str) -> String {
    workspace_root()
        .join(relative)
        .to_string_lossy()
        .to_string()
}
