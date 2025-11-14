ELF (Extensible Lab Framework) — tool tour
===================

Overview
--------
ELF is a Rust-first suite for ECG/EEG/eye data: `elf-cli` exposes signal pipelines, `elf-gui` runs a streaming dashboard, and helper scripts track installers, run bundles, and releases. All tools share `elf-lib` (signal I/O, detectors, HRV/SQI metrics, plot model) so CLI steps and the GUI renderings stay in sync.

CLI quick tour (`elf` binary)
-----------------------------
Use `cargo run -p elf-cli -- <command>` while developing, or build cargo release binaries.

1. **Data ingestion / detectors**
   * `elf ecg-find-rpeaks --fs 250 --input samples.txt`
   * `elf beat-hrv-pipeline --annotations events.tsv --wfdb-header MIT-BIH/100.hea`
   * `elf hrv-time --input test_data/tiny_rr.txt --fs 250`
   * `elf hrv-psd --input test_data/tiny_rr.txt --interp-fs 4`
   * `elf hrv-nonlinear --input test_data/tiny_rr.txt`
   * `elf sqi --input test_data/sqi_sample.txt --fs 250`
2. **Device adapters**
   * `elf bitalino --input test_data/bitalino_sample.csv --signal analog0`
   * `elf open-bci --input test_data/openbci_sample.csv --channel Ch1`
3. **Dataset validation**
   * `elf dataset-validate --spec test_data/dataset_suite_core.json [--update-spec]`
   * Optional `--json out.json` emits structured summaries per dataset case for CI/automation.
   * Add `--update-spec` when you need to recompute fixture metrics and rewrite the spec file to match the new values.
4. **Presenter bundles**
   * `elf run-simulate --design test_data/run_design.toml --trials test_data/run_trials.csv --out /tmp/run`
   * `scripts/generate_run_bundle.sh` reruns the same simulation so validators/release pipelines regenerate the fixture.

User GUI (`elf-gui`)
--------------------
- Multi-tab dashboard (Landing / ECG-HRV / EEG / Eye) that shares the router/store state with the CLI pipelines.
- HRV tab: load raw ECG/WFDB, detect beats, stream synthetic ECG, start/stop Parquet recording, stream LSL devices, and view live SQIs + RR histograms + precomputed PSD/nonlinear metrics.
- HRV tab: the Run bundle controls expose column-name fields (onset/event_type/duration/label) plus a comma-separated event-type filter so you can replay arbitrary bundles.
- HRV tab PSD controls now expose an interpolation-frequency slider (default 4 Hz) so you can tweak the Welch PSD smoothing and recompute LF/HF/VLF summaries for the current RR events.
- Live HRV snapshot shows RMSSD/SDNN/pNN50/LF/HF and now offers an "Export HRV snapshot" button to dump the computed metrics (and optionally RR/events) to JSON.
- The snapshot JSON now tracks the run bundle manifest/filter metadata when available so you can replay the same stimulus context later.
- EEG tab: import EDF channels + BIDS events, peek at event overlays.
- Eye tab: load Pupil/Tobii CSV, adjust confidence thresholds, inspect pupil traces/metrics.
- Shared `Store` caches Figures + dirty flags so only the active tab rebuilds plots.

Scripts
-------
- `scripts/install.sh`: download release tarballs into `~/.local`, verify sha256 via `sha256sum`/`shasum`, and symlink `elf`, `elf-gui`, `elf-run`.
- `scripts/package.sh <version>`: build workspace release, bundle `elf`, `elf-gui`, `elf-run` into `release/elf-<version>-<arch>-<os>.tar.xz`, and emit `*.sha256` files.
- `scripts/generate_run_bundle.sh` (new): reproduce the presenter run bundle plus derived `events.idx` for dataset validation.

Workflows
---------
- `.github/workflows/ci.yml`: on push/PR/release. Runs fmt/clippy/build/test, dataset validation, and drop-in release packaging job for artifact uploads.
- `.github/workflows/release-package.yml`: triggered by release events or manual runs. Runs dataset validation, packages binaries/tarball, computes per-binary SHA256, and publishes everything via `softprops/action-gh-release`.

Release notes & historical records
---------------------------------
Planning/design notes live in the `history/` folder: `PRESENTER_BUNDLES.md`, `LIVE_HRV_UI.md`, `INSTALLER_CI.md`, `DATASET_COVERAGE.md`, `CAKU_NOTES.md`. These document the decisions/traces that led to the current architecture without crowding the root docs.

Test data
---------
- `test_data/` contains ECG fixtures (MIT-BIH, synthetic RR), EEG/eye samples, BITalino/OpenBCI CSVs, dataset specs, and run-simulate artifacts (regenerated on demand).
- `test_data/dataset_suite_core.json` enumerates core regression cases to guard RR, PSD, SQI outputs.

How to contribute
-----------------
1. Use `cargo fmt`, `cargo clippy`, `cargo test`, and `cargo run -p elf-cli -- dataset-validate --spec test_data/dataset_suite_core.json` before pushing.
2. Run `scripts/generate_run_bundle.sh` to regenerate run fixtures whenever you update `run-simulate` or the test design/trials.
3. Update `history/` notes whenever you change planning-level architecture (new dashboards, installers, workflows).
4. Track work via `bd` issues and keep `history/` documenting AI-generated reasoning.
