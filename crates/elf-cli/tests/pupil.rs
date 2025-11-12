use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::error::Error;
use std::path::PathBuf;

#[test]
fn pupil_normalize_outputs_json() -> Result<(), Box<dyn Error>> {
    let mut cmd = cargo_bin_cmd!("elf");
    cmd.args([
        "pupil-normalize",
        "--input",
        &sample_path("test_data/pupil_labs_sample.csv"),
        "--format",
        "pupil-labs",
        "--min-confidence",
        "0.9",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let lines: Vec<&[u8]> = output
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let json: Value = serde_json::from_slice(line)?;
        assert!(json.get("pupil_mm").is_some());
    }
    Ok(())
}

fn sample_path(relative: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join(relative);
    root.to_string_lossy().to_string()
}
