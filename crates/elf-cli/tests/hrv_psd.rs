use assert_cmd::cargo::cargo_bin_cmd;
use serde::Deserialize;
use std::error::Error;
use std::path::PathBuf;

#[derive(Deserialize)]
struct HrvPsdOutput {
    lf: f64,
    hf: f64,
    vlf: f64,
}

fn rr_path() -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("test_data/tiny_rr.txt");
    root.to_string_lossy().to_string()
}

#[test]
fn hrv_psd_command_runs() -> Result<(), Box<dyn Error>> {
    let mut cmd = cargo_bin_cmd!("elf");
    cmd.args(["hrv-psd", "--input", &rr_path(), "--interp-fs", "4"]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let value: HrvPsdOutput = serde_json::from_slice(&out)?;
    assert!(value.lf >= 0.0);
    assert!(value.hf >= 0.0);
    assert!(value.vlf >= 0.0);
    Ok(())
}
