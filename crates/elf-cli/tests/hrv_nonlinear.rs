use assert_cmd::cargo::cargo_bin_cmd;
use serde::Deserialize;
use std::error::Error;
use std::path::PathBuf;

#[derive(Deserialize)]
struct HrvNonlinearOutput {
    sd1: f64,
    sd2: f64,
    samp_entropy: f64,
    dfa_alpha1: f64,
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
fn hrv_nonlinear_command_runs() -> Result<(), Box<dyn Error>> {
    let mut cmd = cargo_bin_cmd!("elf");
    cmd.args(["hrv-nonlinear", "--input", &rr_path()]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let value: HrvNonlinearOutput = serde_json::from_slice(&out)?;
    assert!(value.sd1 >= 0.0);
    assert!(value.sd2 >= 0.0);
    assert!(value.dfa_alpha1 >= 0.0);
    assert!(value.samp_entropy >= 0.0);
    Ok(())
}
