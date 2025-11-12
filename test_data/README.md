# Test Recordings

This directory contains lightweight assets used by `elf-cli` integration tests.

- `synthetic_recording_a.txt` — simulated single-lead ECG sampled at 250 Hz with realistic RR variability and noise. Each line is one sample (volts).  Use `elf beat-hrv-pipeline --fs 250 --input test_data/synthetic_recording_a.txt` to smoke test the full pipeline.
- `synthetic_recording_a_expected.json` — reference RR intervals and HRV metrics computed directly from the synthetic ground-truth beat times. Tests compare the CLI output against this file with tight tolerances.

The remaining markdown notes describe future external datasets to plug in when available.

## EEG sample events

- `bids_sample.tsv` — toy `events.tsv` mimicking a BIDS run; used to test the new BIDS event parser in `elf-lib::io::eeg`.  The CLI can load it via `--bids-events test_data/bids_sample.tsv` while inspecting any waveform, and the parser converts the onset times to beat indices at the requested sampling rate.

## Eye-tracking samples

- `eye_sample.csv` — small CSV with left/right pupil diameters (used by the eye reader tests).
- `pupil_labs_sample.csv` / `tobii_sample.tsv` — vendor-style exports demonstrating the column names and confidence filtering supported by `elf-lib::io::eye` and the `elf pupil-normalize` CLI helper.

## MIT-BIH Sample

- `mitdb/100.dat`, `mitdb/100.hea`, `mitdb/100.atr` — a small slice of the MIT-BIH Arrhythmia DB used to test the WFDB loaders. Run `cargo run -p elf-cli -- beat-hrv-pipeline --wfdb-header test_data/mitdb/100.hea --wfdb-lead 0 --annotations test_data/mitdb/100.atr` (or `--annotations` with a text file of beat indices) to exercise the new format handlers.
