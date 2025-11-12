# 0) High-level goals

* **Scriptable first**: every processing pipeline is runnable from the CLI, deterministic via explicit seeds/thresholds, and compatible with shell pipelines (`stdin`/`stdout`).
* **Deterministic numerics**: use `f64` throughout HRV/SQI computations, surface tolerances in regression specs, and mirror reference implementations (pyHRV, PhysioNet) when possible.
* **Device-agnostic ingest**: readers for WFDB/EDF, BITalino/OpenBCI CSV, Pupil/Tobii exports, synthetic fixtures, and any new adapter should feed the same signal/plot abstractions.
* **Shared visualization model**: keep the CLI plot outputs and GUI figures in sync via the `elf-lib::plot` helpers, so PNGs and `egui_plot` renderings stay consistent.
* **Streaming + background compute**: acquisition, detection, and costly metrics run on worker threads, while GUI tabs paint cached snapshots to stay responsive.

---

# 1) Crates in the workspace

* `elf-lib` is the portable DSP core: signal I/O for CSV/Polars/WFDB/EDF/BIDS/eye exports, Pan–Tompkins detectors, HRV metrics (time/frequency/nonlinear), SQIs, and the shared `Figure/Series` plot model that CLI + GUI share.
* `elf-cli` wires `clap` subcommands (`ecg-find-rpeaks`, `beat-hrv-pipeline`, `hrv-time`, `hrv-psd`, `hrv-nonlinear`, `sqi`, `bitalino`, `openbci`, `dataset-validate`, `hrv-plot`, etc.) to the core. Everything accepts either raw traces, annotations, or RR lists.
* `elf-gui` runs on `eframe/egui`: a `StreamingStateRouter` processes ECG chunks through `run_beat_hrv_pipeline`, records to Parquet, and feeds the shared `Store` that decimates figures, evaluates SQIs, and caches snapshots for each tab (ECG/EEG/Eye).
* `elf-web` hosts reproducible docs, datasets, and eventually a thin SaaS layer that reuses the same signal abstractions.

---

# 2) Data ingestion & validation

* Inputs: newline-delimited samples, WFDB `.hea/.dat/.atr`, EDF, BIDS event TSVs, BITalino/OpenBCI CSVs, Pupil/Tobii exports, and synthetic fixtures from `test_data/` (MIT-BIH, RR gold, pupil samples, etc.).
* `elf-lib::io` exports helper loaders for each modality so CLI commands share them; `elf-gui` reuses the same functions when loading files from the UI panels.
* `elf-cli dataset-validate --spec test_data/dataset_suite_core.json` recomputes metrics for stored fixtures and enforces tolerances (RR/HRV, annotations, SQIs) as part of CI.
* Output bundles: CLI `hrv-plot` renders PNGs using `elf-lib::plot`, while `elf-gui` caches `Figure` objects and decimates them with `decimate_points`, keeping live views fast.

---

# 3) Streaming & synchronization

* `StreamingStateRouter` owns a `Store`, a worker thread, command/update channels, and an optional `ParquetRecorder`.
* Incoming `StreamCommand::ProcessEcg` or `IngestEvents` get processed off the UI thread; events and HRV metrics are sent back as `StreamUpdate` messages (`Ecg`, `Events`, `Hrv`), so the Store only recomputes figures/metrics when dirty.
* The worker reuses `EcgPipelineConfig::default()` and runs `run_beat_hrv_pipeline`, so GUI streaming and CLI pipelines stay in sync; synthetic streaming uses the same router (`StreamingSimulator`).
* Recording controls write ECG chunks to Parquet; `Store::prepare_active_tab` drains pending updates before recomputing figures, RR series, PSD, nonlinear metrics, and SQIs.

---

# 4) Runtime architecture

```
ElfApp (eframe)
├─ StreamingStateRouter (worker thread)
│   ├─ command_tx: StreamCommand (ECG chunks/events)
│   ├─ update_rx: StreamUpdate (Ecg/Events/Hrv)
│   └─ ParquetRecorder (optional)
├─ Store (snapshot + DirtyFlags)
│   ├─ ECG trace + decimated Figure
│   ├─ Events → RRSeries → HRV + PSD + nonlinear + SQI
│   └─ Eye samples → filtered series + metrics (kept ratio, per-eye stats)
└─ UI tabs (Landing / Hrv / EEG / Eye)
```

* The UI never blocks: all heavy work happens in the router worker, `Store::prepare` lazily rebuilds only the dirty pieces, and each tab renders cached `Figure` data.
* Tab-specific state (zoom window, event drag/hover, thresholds) lives inside `ElfApp` but reads from the shared `Store` for signal + metric snapshots.
* Device imports (WFDB, EDF, BITalino, OpenBCI, Pupil/Tobii) go through the same pipeline so GUI actions and CLI commands remain consistent.

---

# 5) Key CLI commands (first wave)

```bash
elf ecg-find-rpeaks --fs 250 --input samples.txt
elf beat-hrv-pipeline --lowcut 5 --highcut 15 --annotations events.atr
elf hrv-time --input test_data/tiny_rr.txt
elf hrv-psd --input test_data/tiny_rr.txt --interp-fs 4
elf hrv-nonlinear --input test_data/tiny_rr.txt
elf sqi --input test_data/sqi_sample.txt
elf bitalino --input test_data/bitalino_sample.csv --signal analog0
elf openbci --input test_data/openbci_sample.csv --channel Ch1
elf dataset-validate --spec test_data/dataset_suite_core.json
elf hrv-plot --input test_data/tiny_rr.txt --out /tmp/hrv.png
```

* Each command can consume raw traces, RR lists, or annotations; detectors and metrics live in `elf-lib` so results are consistent across targets.
* Add new commands as wrappers around the shared pipelines (e.g., future `pupil-normalize`), and keep regression fixtures under `test_data/` for CI.

---

# 6) GUI workflows

* **ECG/HRV tab**: load ECG or WFDB, detect beats (drag/reposition as needed), stream synthetic ECGs, record Parquet chunks, and visualize HRV metrics + PSD / nonlinear numbers.
* **EEG tab**: load EDF channels, import BIDS events, drag events directly on the trace, and export the event list.
* **Eye tab**: import Pupil/Tobii CSV, adjust confidence threshold, inspect pupil decimated figure, add/remove gaze events, and view derived metrics (kept ratio, per-eye stats). Metrics are cached in `Store` so UI stays responsive.
* All tabs share the same `Store` snapshot and only recompute when dirty flags signal an update.

---

# 7) Event model & shared APIs

* `elf-lib::signal::{TimeSeries, Events, RRSeries}` describe ECG/pupil data and beat indices; converters keep CLI and GUI synchronized.
* `elf-lib::plot::{Figure, Series, Style}` are reused by `elf-cli hrv-plot` (Plotters backend) and `elf-gui` (`egui_plot`).
* `elf_lib::metrics::hrv` exposes `hrv_time`, `hrv_psd`, `hrv_nonlinear`, and `evaluate_sqi`, all deterministic and validated via tests (`test_data`).
* Future adapters (new data sources, metrics, or plot styles) should live in `elf-lib` so every target benefits automatically.

---

# 8) Validation & next steps

* `test_data/` hosts WFDB samples, synthetic RR, pupil exports, BITalino/OpenBCI CSVs, and dataset suites (see `dataset_suite_core.json`).
* CI runs `cargo fmt`, `cargo clippy`, `cargo test`, and `cargo run -p elf-cli -- dataset-validate --spec test_data/dataset_suite_core.json` to catch regressions.
* Next stretch: keep adding device adapters, SQIs, and streaming helpers, but continue to expose the same deterministic pipelines in CLI + GUI so tests/data in `test_data/` capture every new path.
