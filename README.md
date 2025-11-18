![image](./screenshot.png)
# ELF — Extensible Lab Framework

`elf` is a Rust-native, local-first toolkit for acquiring, processing, and visualizing physiological signals. It bundles a portable DSP core (`elf-lib`), a CLI entrypoint (`elf`), an egui dashboard (`elf-gui`), and supporting tools for EEG/eye tracking and streaming adapters. The focus is on transparent HRV analytics, extensible importers (WFDB, EDF, BITalino, OpenBCI, CSV, BIDS), and consistent plotting/validation across CLI + GUI targets.

### Documentation
Generated Rust API/docs live under `target/doc/` after you run `cargo doc`. For quick browsing, open `target/doc/elf/index.html` (or `cargo doc --open`) to review the CLI/core reference, and check `target/doc/elf_gui/index.html` for the dashboard-level API once you build the workspace docs.

## User manual overview

1. Install the suite with the provided installers or build from source.
2. Choose the CLI workflow (beat detection, HRV math, run-simulation) that matches your data modality.
3. When you need a GUI, load the same run bundles or streams to visualize beats, PSD, and SQI charts.
4. Extend `test_data/` with new fixtures and add them to `test_data/dataset_suite_core.json` so regression checks guard your pipeline.

---

## Installation & setup

### Build from source

```bash
git clone https://github.com/monistowl/elf.git
cd elf
cargo build --workspace
```

`cargo build --release` followed by `scripts/package.sh` produces the tarball + SHA256 that `scripts/install.sh` expects. Run `scripts/install.sh` (default `BASE_URL=https://example.com/elf/releases`) to deploy binaries into `~/.local/opt/elf/<version>` with symlinks in `~/.local/bin`. Remove the installation by running `scripts/uninstall.sh`.

### Smoke test the CLI

After installation, try the following quick commands:

```bash
cat test_data/synthetic_recording_a.txt | elf -- ecg-find-rpeaks --fs 250 | jq
elf -- hrv-time --input test_data/tiny_rr.txt --fs 250 | jq
elf -- hrv-psd --input test_data/tiny_rr.txt --interp-fs 4 | jq
elf -- hrv-nonlinear --input test_data/tiny_rr.txt | jq
elf -- run-simulate --design test_data/run_design.toml --trials test_data/run_trials.csv --out /tmp/run
```

`run-simulate` honors the STIMULUS_PRESENTER manifest, writes `events.tsv`/`events.json`, and emits `run.json` so the GUI can replay the same bundle.

---

## CLI command reference

### `elf ecg-find-rpeaks`
Detect R-peaks from newline-delimited samples. Required flags:

- `--fs <Hz>`: sampling frequency of the signal.
- `--input <path>` or `--wfdb-header <path>` plus `--wfdb-lead`: source data.

Optional tuning:

- `--min-rr-s`: refractory period in seconds (default 0.12).
- `--threshold-scale`: adaptive threshold multiplier.
- `--search-back-s`: how far back to scan for peak refinement.
- `--annotations`/`--bids-events`: skip detection and use provided beat indices.

### `elf beat-hrv-pipeline`
Runs the detector, RR conversion, and HRV summaries in one shot. Defaults work for ambulatory ECGs, but every parameter can be overridden (`lowcut-hz`, `highcut-hz`, `integration-window-s`, `min-rr-s`, `threshold-scale`, `search-back-s`). Supply waveform inputs or annotations as with `ecg-find-rpeaks`.

Examples:

```bash
elf -- beat-hrv-pipeline --wfdb-header test_data/mitdb/118.hea --wfdb-lead 0 --annotations test_data/mitdb/118.atr
elf -- beat-hrv-pipeline --fs 250 --input test_data/synthetic_recording_a.txt --bids-events test_data/bids_sample.tsv
```

### HRV helper commands

- `elf hrv-time --input <rr.txt>`: compute AVNN/SDNN/RMSSD/pNN50.
- `elf hrv-psd --input <rr.txt> --interp-fs 4`: Welch PSD across VLF/LF/HF bands (interpolation defaults to 4 Hz).
- `elf hrv-nonlinear --input <rr.txt>`: Poincaré `sd1`/`sd2`, sample entropy, and DFA α1.

### `elf run-simulate`
Simulate TRIALS + DESIGN manifests to produce presentation-ready bundles. Supply `--design`, `--trials`, `--sub`, `--ses`, `--run`, and `--out`. The generated folder contains `events.tsv`, `events.json`, and `run.json` for GUI replay.

### `elf dataset-validate`
Recomputes metrics from `test_data/dataset_suite_core.json` and compares them to stored tolerances. Add new fixtures plus expected metrics when you add datasets to keep CI reproducible. Run the same command with `--update-spec` to recompute and rewrite the stored metrics whenever new fixtures or pipeline changes require refreshed tolerances.

### `elf pupil-normalize`
Parses the provided CSV/TSV, filters on `confidence`, and emits JSON per sample. Use `--format {pupil-labs|tobii}` to pick column mappings and `--min-confidence` to drop noisy samples.

---

## Example workflows

1. **ECG QA**: detect beats from a MIT-BIH record, export RR intervals, run `elf beat-hrv-pipeline --annotations` for historical HRV comparisons, and save the summary JSON for downstream monitoring.
2. **Presenter bundle**: run `elf run-simulate` → open `elf-gui` → click “Load run bundle” → share the `events.tsv`/`run.json` directory so the GUI plots the same metrics and shows ISI/jitter metadata.
3. **Brain/eye preprocessing**: `elf beat-hrv-pipeline --bids-events <events.tsv>` converts BIDS onset files into HRV stats; run `elf pupil-normalize --format pupil-labs --min-confidence 0.9` before feeding samples into SQI algorithms.

---

## MCP & security overview

`elf-mcp` now ships with the suite so agents or transport clients can hit your tooling over the Model Context Protocol. Run direct probes or start the stdio transport:

```bash
cargo run -p elf-mcp -- --transport stdio catalog-summary
cargo run -p elf-mcp -- --transport stdio run-tool --name simulate_run --params '{"design":"test_data/run_design.toml","trials":"test_data/run_trials.csv"}'
echo '{"id":"1","method":"list_bundles","params":{}}' | cargo run -p elf-mcp -- --transport stdio serve
```

Key/cert bundles live under `~/.config/elf-mcp/keys` (use `ELF_KEY_DIR` to override). Manage them with the integrated helper:

```bash
cargo run -p elf-mcp -- key list
cargo run -p elf-mcp -- key generate --name operator --days 365
cargo run -p elf-mcp -- key import --name operator --cert /tmp/operator.cert.pem --key /tmp/operator.key.pem
cargo run -p elf-mcp -- key export --name operator --dest /tmp
```

The GUI now exposes a “Security” tab where you can refresh stored bundles, generate new PEM pairs, import existing cert/key files, and export bundles for transport clients that need TLS material.

---

## GUI quick tour

```bash
elf-gui
```

The dashboard shares the same state/metrics as the CLI. Load ECG inputs, annotations, or run bundles in the HRV tab, and the shared `Store` ensures plots/figures stay in sync across controls. The new `Load run bundle` button points to a bundle directory (`events.tsv` + `run.json`), surfaces manifest stats (ISI, jitter, policy), and feeds the shared `Store` so CLI + GUI outputs look the same.

The HRV tab also exposes a PSD interpolation slider (default 4 Hz) that lets you tweak the Welch PSD interpolation rate and immediately recompute the plotted LF/HF/VLF power for the beats or streamed events you already loaded.

Run bundle loading now lets you override the TSV column names (onset/event_type/duration/label) and supply a comma-separated list of event types so you can load bundles that expose different column headers or event names without editing source code.

The Live HRV snapshot panel now includes an "Export HRV snapshot" button so you can capture the current RR/PSD/time metrics as JSON for archiving or regression tests.
The exported JSON now bundles the run manifest/filter metadata (if you loaded a bundle) so the exact stimulus context travels with the snapshot.

---

## TUI dashboard

```bash
cargo run -p elf-tui
```

`elf-tui` mirrors the GUI tabs in a terminal-only flow powered by `ratatui`. Use the left/right arrows (or `1`–`3`) to pick tabs, `Tab` to edit fields, and `Enter` to run the ECG → HRV pipeline or load a run bundle for manifest/event summaries.

---

## Validation & testing

- `cargo test` covers `elf-lib` metrics, CLI regression suites (`pipeline.rs`, `pupil.rs`, etc.), and the dataset validator.
- CI runs `cargo fmt`, `cargo clippy`, `cargo build`, `cargo test`, and `elf -- dataset-validate --spec test_data/dataset_suite_core.json`.
- Extend `test_data/` with new RR lists, WFDB records, BIDS events, run bundles, or eye exports and add entries to `dataset_suite_core.json` to lock state.

---

## Installer helpers

`install.sh` downloads a release tarball from `BASE_URL` (default `https://example.com/elf/releases/<version>`), verifies the SHA256, extracts into `~/.local/opt/elf/<version>`, and symlinks `elf`, `elf-gui`, `elf-run`. `scripts/package.sh` runs in CI for every release. `scripts/uninstall.sh` removes the symlinks and `current` pointer.

---

## Contributing

Follow `AGENTS.md`: use `bd` for issue tracking, keep planning docs in `history/`, and update the `test_data/` regression fixtures whenever you adjust detectors, HRV math, or validation tolerances.
