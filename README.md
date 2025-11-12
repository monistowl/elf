![image](./screenshot.png)
# ELF — Extensible Lab Framework

`elf` is a Rust-native, local-first toolkit for acquiring, processing, and visualizing physiological signals. It bundles a portable DSP core (`elf-lib`), a CLI entrypoint (`elf`), an egui dashboard (`elf-gui`), and supporting tools for EEG/eye tracking and streaming adapters. The focus is on transparent HRV analytics, extensible importers (WFDB, EDF, BITalino, OpenBCI, CSV, BIDS), and consistent plotting/validation across CLI + GUI targets.

---

## Getting started

### Build

```bash
cargo build --workspace
```

### CLI quick tour

Install the suite with `scripts/install.sh` (it unpacks release tarballs into `~/.local/opt/elf/<version>` and symlinks `elf`, `elf-gui`, and `elf-run` into `~/.local/bin`). Once `elf` is on your PATH you can run the headless tools without `cargo run`:

```bash
cat test_data/synthetic_recording_a.txt | elf -- ecg-find-rpeaks --fs 250 | jq
elf -- hrv-time --input test_data/tiny_rr.txt --fs 250 | jq
elf -- hrv-psd --input test_data/tiny_rr.txt --interp-fs 4 | jq
elf -- hrv-nonlinear --input test_data/tiny_rr.txt | jq
elf -- run-simulate --design test_data/run_design.toml --trials test_data/run_trials.csv --out /tmp/run
```

The `run-simulate` helper reads the TOML/CSV pair described in `STIMULUS_PRESENTER.md`, schedules trials (with optional jitter and randomization policy), emits `events.tsv`/`events.json`, and writes a `run.json` manifest so the GUI can load the exact bundle later.

### GUI quick tour

```bash
elf-gui
```

Load raw ECGs or WFDB/EDF spectra, import annotations, stream synthetic beats, or point the new `Load run bundle` button at a directory containing `events.tsv` + `run.json`. The UI reuses the shared `Figure` model so CLI plots, GUI graphs, and streaming downsampling stay consistent.

---

## Architecture overview

- `elf-lib`: signal I/O for CSV/WFDB/EDF/eye exports, detectors, HRV/SQI metrics, and the shared `Figure/Series` plot model.
- `elf-cli`: the `elf` binary exposing commands (`ecg-find-rpeaks`, `beat-hrv-pipeline`, `hrv-time`, `hrv-psd`, `hrv-nonlinear`, `sqi`, `dataset-validate`, `run-simulate`); bundles now include installer-friendly scripts.
- `elf-gui`: `eframe` dashboard with `StreamingStateRouter`, `Store`, and run-bundle loaders so the same events feed all tabs.

---

## Validation & testing

- `cargo test` covers CLI/lib regression suites, SQI, PSD, nonlinear metrics, and the new run-simulate command.
- CI runs `cargo fmt`, `cargo clippy`, `cargo build`, `cargo test`, and `elf -- dataset-validate --spec test_data/dataset_suite_core.json`.
- Extend `test_data/` plus `dataset_suite_core.json` whenever you add new fixtures (RR, WFDB, BIDS, BITalino, run specs).

---

## Installer helpers

`install.sh` lives under `scripts/install.sh`. It downloads a release tarball from `BASE_URL` (default `https://example.com/elf/releases/<version>/elf-<version>-<arch>-<os>.tar.xz`), verifies the SHA256, extracts into `~/.local/opt/elf/<version>`, updates the `current` symlink, and links `elf`, `elf-gui`, and `elf-run` into `~/.local/bin`. Run `scripts/uninstall.sh` to remove the symlinks and `current` pointer.

---

## Contributing

Follow the `AGENTS.md` conventions: use `bd` for tracking, keep ephemeral docs under `history/`, and add regression fixtures under `test_data/`. When you add new datasets run `elf -- dataset-validate` to keep the golden comparison up to date.
