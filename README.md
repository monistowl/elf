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

# run GUI
cargo run -p elf-gui
```

The CLI now understands WFDB records. Point `--wfdb-header` at a `.hea` file, `--wfdb-lead` at the desired channel, and `--annotations` at the `.atr` or newline list to reuse the same detector/HRV pipeline when working with PhysioNet data.
Support for EEG-like formats is also landing: use `--eeg-edf`/`--eeg-channel` to read EDF files, and pass `--bids-events` alongside your pipeline to convert BIDS `events.tsv` onsets into sample indices while reusing the shared HRV tooling.
Use `elf-cli pupil-normalize` with any of the supported column presets (`pupil-labs` or `tobii`) to filter pupil exports by confidence and emit normalized JSON so downstream tools (e.g., blink/interpolation pipelines) can read them.

The GUI now includes controls for pointing at a raw ECG recording (newline-delimited samples) and an optional
annotation file, or for invoking the built-in detector when only the raw waveform is available. Uploaded beats
are embedded into the plot and HRV summary tiles are updated from the same `elf-lib` pipeline that powers the CLI.

## Next steps
- Replace naive R-peak picker with proper pipeline (bandpass, diff, square, MWI, adaptive threshold).
- Add Welch PSD and nonlinear HRV in `elf-lib::metrics`.
- Add CSV/Parquet readers (enable `elf-lib` feature `polars`).
- Wire live streaming (LSL/OpenBCI adapters) and plots in `elf-gui`.

### New CLI helpers
- `elf hrv-psd --input test_data/tiny_rr.txt --interp-fs 4` computes Welch PSD-derived LF/HF/VLF metrics (JSON).
- `elf hrv-nonlinear --input test_data/tiny_rr.txt` prints Poincaré + sample entropy stats.
