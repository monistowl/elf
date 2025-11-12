use assert_cmd::Command;
use serde_json::Value;
use std::{fs, path::PathBuf};
use tempfile::tempdir;

#[test]
fn run_simulate_writes_bundle() {
    let temp = tempdir().unwrap();
    let out = temp.path().join("runs/sub-01/ses-01/task-stim_run-01");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let design = workspace_root.join("test_data/run_design.toml");
    let trials = workspace_root.join("test_data/run_trials.csv");
    Command::cargo_bin("elf-cli")
        .unwrap()
        .args(&[
            "run-simulate",
            "--design",
            design.to_str().unwrap(),
            "--trials",
            trials.to_str().unwrap(),
            "--sub",
            "01",
            "--ses",
            "01",
            "--run",
            "01",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let events = out.join("events.tsv");
    assert!(events.exists());
    let contents = fs::read_to_string(&events).unwrap();
    assert!(contents.contains("stim"));
    let manifest = out.join("run.json");
    let json: Value = serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(json["task"], "stroop");
    assert_eq!(json["design"], "stroop");
}
