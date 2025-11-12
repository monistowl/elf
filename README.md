# ELF — Extensible Lab Framework (bootstrap)

Local-first physiologic signal processing + biofeedback toolkit in Rust.

## Quickstart

```bash
# build all
cargo build --workspace

# run CLI
cat ecg.txt | cargo run -p elf-cli -- ecg-find-rpeaks --fs 250 | jq
cat rr.txt  | cargo run -p elf-cli -- hrv-time | jq
# run the end-to-end beat→RR→HRV pipeline on a sample recording
cargo run -p elf-cli -- beat-hrv-pipeline --fs 250 --input test_data/synthetic_recording_a.txt | jq
cargo run -p elf-cli -- hrv-psd --input test_data/tiny_rr.txt --interp-fs 4 | jq
cargo run -p elf-cli -- hrv-nonlinear --input test_data/tiny_rr.txt | jq

# run GUI
cargo run -p elf-gui
```

The CLI now understands WFDB records. Point `--wfdb-header` at a `.hea` file, `--wfdb-lead` at the desired channel, and `--annotations` at the `.atr` or newline list to reuse the same detector/HRV pipeline when working with PhysioNet data.
Support for EEG-like formats is also landing: use `--eeg-edf`/`--eeg-channel` to read EDF files, and pass `--bids-events` alongside your pipeline to convert BIDS `events.tsv` onsets into sample indices while reusing the shared HRV tooling.
Use `elf-cli pupil-normalize` with any of the supported column presets (`pupil-labs` or `tobii`) to filter pupil exports by confidence and emit normalized JSON so downstream tools (e.g., blink/interpolation pipelines) can read them.
The new `elf-cli run-simulate` command reads the TOML/CSV pair described in `STIMULUS_PRESENTER.md`, schedules each trial (with optional jitter/rand policy) into `events.tsv`/`events.json`, and writes a `run.json` manifest so GUI dashboards can load the same bundle.

The GUI now includes controls for pointing at a raw ECG recording (newline-delimited samples) and an optional
annotation file, or for invoking the built-in detector when only the raw waveform is available. Uploaded beats
are embedded into the plot and HRV summary tiles are updated from the same `elf-lib` pipeline that powers the CLI.
It can also load a run bundle directory (the `events.tsv`/`run.json` produced by `run-simulate`) so you can inspect dataset-level metadata, run times, and event jitter before rerunning detection.

## Next steps
- Replace naive R-peak picker with proper pipeline (bandpass, diff, square, MWI, adaptive threshold).
- Add CSV/Parquet readers (enable `elf-lib` feature `polars`).
- Wire live streaming (LSL/OpenBCI adapters) and plots in `elf-gui`.

### Streaming router
The GUI now routes incoming ECG/annotation chunks through a `StreamingStateRouter` that uses a `crossbeam-channel` worker to compute PSD/nonlinear HRV off the UI thread before publishing snapshots to whichever tab is active.
You can try the new "Stream synthetic beats" button on the ECG/HRV tab to inject a small batch of reference RR events and see the worker compute metrics without locking the UI; there is also a "Process synthetic ECG" button that feeds the shared synthetic trace into the worker so detection + HRV happen off the UI thread.

### New CLI helpers
- `elf hrv-psd --input test_data/tiny_rr.txt --interp-fs 4` computes Welch PSD-derived LF/HF/VLF metrics plus a dense PSD trace (JSON).
- `elf hrv-nonlinear --input test_data/tiny_rr.txt` reports Poincaré SD1/SD2, sample entropy, and DFA alpha1.
