# Architecture sketch (monorepo)

* `elf-lib` (no_std-lean where possible)

  * Signal I/O: CSV/Parquet/Arrow ingestion + writers (Polars/Arrow) plus WFDB/EDF/BIDS/Eye adapters for raw ECG/EEG and pupil exports.
  * Core DSP: filtering (IIR/biquad), resampling, windowing, FFT/Welch PSD.
  * ECG/PPG: peak detection (Pan-Tompkins style & adaptive alternatives), beat-to-beat series, SQI, artifact handling.
  * HRV metrics: time, frequency, nonlinear, Poincaré, DFA.
  * Plot model: shared `Figure`/`Series` + decimation helpers keep CLI PNGs and egui panels in sync without dragging GUI deps into `elf-lib`.
  * Streaming: LSL bindings for live sensors.
* `elf-cli` (clap)

  * Unixy subcommands: `ecg-find-rpeaks`, `rr-clean`, `beat-hrv-pipeline`, `hrv-time`, `hrv-psd`, `hrv-nonlinear`, `hrv-plot`, `pupil-normalize`, `ppg-clean`, `sqi`, etc. Commands can read WFDB headers, EDF/BIDS traces, or pupil CSV/TSV before piping into the shared `Figure` model and PNG exports, and observed test_data (MIT-BIH, synthetic RR, pupil/Tobii samples) already serve as regression fixtures.
* `elf-gui` (egui/eframe)

  * Multi-tab dashboard (landing / ECG-HRV / EEG / eye-tracking) that mirrors the CLI plot model, lets you load raw ECGs or WFDB headers, import .atr/BIDS/CSV annotations, detect beats, and drop into dedicated EEG/eye tracking tabs, all while keeping per-tab `egui_plot` nodes decimated and cached.
* `elf-web`

  * Static docs + sample datasets, later a SaaS thin layer. Keep it separate from the local-first flow.

Key Rust crates to lean on:

* GUI/Wasm: `egui` + `eframe` (native + Wasm in one codebase). ([GitHub][1])
* DSP: `rustfft` / `realfft` for FFT & Welch PSD; `biquad`/`iir_filters` for filters; `cpal`/`rodio` if you experiment with audio biofeedback. ([crates.io][2])
* Dataframes & columnar: Polars + Arrow2 for fast tabular I/O and IPC. ([Docs.rs][3])
* Live streaming: LSL via `liblsl-rust` for plug-and-play with lots of devices/labs. ([GitHub][4])

---

# Milestones (sequenced so you can “dogfood” quickly)

**M0 — Repo bootstrap (1–2 days)**

* Workspace with 4 crates; `elf-lib` skeleton (traits: `Signal`, `Detector`, `Metric`).
* CI: fmt, clippy, tests on Linux/macOS/Windows; Wasm build for `elf-gui`.

**M1 — HRV core, batch mode (1–2 weeks)**

* Implement r-peaks (ECG) & peaks (PPG), RR series, artifact removal, time-domain HRV (SDNN, RMSSD, pNN50).
* CLI: `ecg-find-rpeaks`, `rr-from-rpeaks`, `hrv-time`.
* Validate against PhysioNet RR datasets. ([PhysioNet][5])

**M2 — Frequency & nonlinear HRV (1–2 weeks)**

* Welch PSD; LF/HF/VLF; Poincaré (SD1/SD2); DFA, sample entropy.
* CLI: `hrv-psd`, `hrv-nonlinear`.

**Status (November 12, 2025):** Welch PSD + LF/HF/VLF integration now lives in `elf-lib::metrics::hrv`, feeding both the CLI (`hrv-psd`) and the GUI frequency plots. Nonlinear metrics expose SD1/SD2, sample entropy, and DFA alpha1 via `hrv-nonlinear`, with regression tests guarding the math.

**M3 — Live streaming & dashboard (2–3 weeks)**

* LSL subscribe, record to Parquet; egui dashboard: live plots, bandpass options, “compute HRV live” panel.
* Optional audio biofeedback (RMSSD → tone via `cpal`).

**Status (November 12, 2025):** Introduced a `StreamingStateRouter` inside `elf-gui` that queues ECG/annotation chunks using `crossbeam-channel` and lets a worker compute RR/PSD/nonlinear metrics before routing snapshots to the active tab, keeping heavy math off the UI thread.
The HRV tab exposes a "Stream synthetic beats" button so you can exercise this background pipeline with reference RR chunks without needing live hardware.
Processing the synthetic ECG recording now funnels the entire waveform through the router worker, so beat detection and HRV calculations happen on a background thread rather than blocking the GUI.

**M4 — Device adapters & SQIs (ongoing)**

* Readers for OpenSignals/BITalino, OpenBCI, CSV. Add SQIs for ECG/PPG quality. ([GitHub][6])

---

## Validation snapshots & future stretch goals

* `test_data/` carries real samples—MIT-BIH `100.{hea,dat,atr}` for R-peak validation, `tiny_rr.txt` and synthetic RR gold outputs, BIDS `events.tsv`, and Pupil/Tobii eye exports—so CLI tests/GUI filters already exercise the formats we advertise.
* `plot` module in `elf-lib` now serves both `elf-cli` (PNG `hrv-plot`) and `elf-gui` (egui plots) so new dashboards can reuse the same decimated series without recreating buffers.
* Next stretch goals: wire `elf-gui` tabs to live `Store` snapshots, surface more derived metrics per modality (EEG epochs, pupil dilation), and keep the multi-target builds (linux/x11, macOS, Windows, WASM) tidy.

## Next focus

1. **M3 streaming & state router** — finish LSL ingest + Parquet recording, add a reducer that feeds snapshots to whichever tab is visible, and keep worker threads computing PSD/nonlinear HRV off the UI thread.
2. **Presenter bundles** — add the `run-simulate` command that normalizes design/TOML + trial/CSV specs into events/manifest bundles, track jitter/randomization metadata, and let `elf-gui` load the same bundle to replay stimuli, show manifest stats, and feed the shared `Store` with consistent `Events`/RRs.
3. **Dataset coverage & automation** — expand PhysioNet/BIDS fixtures, add automated validation (golden RR + PSD comparisons) for each format, and keep new data in `test_data/` to lock regressions.
4. **CLI + installer** — publish the shared `elf` binary (via `[[bin]] name = "elf"` in `crates/elf-cli/Cargo.toml`), keep release tarballs with `elf`, `elf-gui`, and `elf-run`, and maintain `scripts/install.sh`/`scripts/uninstall.sh` that push symlinks into `~/.local/bin` so future work can rely on the CLI being on PATH.
5. **UI polish** — stabilize tab interactions, add zoom/selection/decimation controls that reuse the shared `Figure`, cache derived curves across tabs, and surface more HRV/EEG/eye metrics in the dashboard.

# High-quality open repos to mine/port (starter list)

These are battle-tested codebases (mostly Python/C/C++) with algorithms you can re-implement cleanly in Rust. I’ve grouped them by what they’re best for.

### General biosignal toolboxes

* **WFDB / PhysioNet tools** — classic C toolkit & formats; dozens of algorithms and utilities; great reference for readers/writers and signal ops. ([PhysioNet][7])
* **BioSPPy** — compact biosignal toolbox (ECG/PPG/EDA/RSP/EEG) with filtering, peak-finding, feature extraction; good for mapping APIs to `elf-lib`. ([GitHub][8])
* **NeuroKit2** — wide coverage of ECG/PPG/EDA/EMG/RSP, with solid SQI and end-to-end recipes; excellent test oracles for your Rust ports. ([GitHub][9])

### HRV-focused

* **HeartPy** — robust peak detection and HRV on PPG/ECG; also has Arduino sketches showing embedded constraints. ([GitHub][10])
* **pyHRV** — clean separation of time/frequency/nonlinear HRV metrics; mirrors literature definitions; good parity target for `elf-cli`. ([GitHub][11])

### Data & readers

* **PhysioNet RR interval sets** — healthy, CHF, CAST etc. for validation and CI regression. ([PhysioNet][5])
* **BITalino OpenSignals reader & sample data** — spec + examples to ensure `elf-lib` can open labs’ CSV/HDF5. ([GitHub][6])
* **MNE-Python** (EEG/MEG) — if you later branch into ERPs/EEG; not HRV, but great I/O & preprocessing reference. ([GitHub][12])
* **MIT-BIH & BIDS eye/EEG samples** — track the test data samples we already mirror (MIT-BIH ECG `.hea/.dat/.atr`, Pupil/Tobii CSV/TSV, BIDS events) for validation and regression.

### Streaming

* **LabStreamingLayer (LSL)** core + apps (LabRecorder) and **Rust bindings** — your shortest path to live devices and cross-app sync. ([GitHub][13])
* **OpenBCI** libs (for EEG/ECG hardware) if you want native drivers beyond LSL. ([GitHub][14])

### rPPG (optional but fun)

* **pyVHR** — video-based HR from webcam; great stretch goal for the GUI (camera → rPPG → HRV). ([GitHub][15])

---

# First ports to tackle (bite-sized)

1. **Peak detection (ECG & PPG)**

   * Pan-Tompkins-style pipeline (bandpass → diff → square → moving window integration + adaptive threshold). Validate on PhysioNet.
   * Add PPG-specific peak picking + motion artifact guards (borrow SQI ideas from NeuroKit2). ([SpringerLink][16])

2. **RR cleaning & SQI**

   * Implement outlier rules + local median filters; compute ECG/PPG SQIs and tag segments (good/uncertain/bad). ([neuropsychology.github.io][17])

3. **HRV Time & Freq**

   * Time: AVNN, SDNN, RMSSD, pNN50/20, TINN.
   * Freq: Welch PSD with exact LF/HF integration options; report absolute, normalized, LF/HF. (Use `realfft` + Hanning windows.) ([crates.io][18])

4. **Nonlinear**

   * Poincaré SD1/SD2, SampEn/ApEn, DFA. (Mirror pyHRV behavior/params.) ([GitHub][11])

5. **CLI ergonomics**

   * `cat ecg.csv | elf ecg-find-rpeaks --fs 250 --col v1 | elf rr-from-rpeaks | elf hrv-time --json`

6. **GUI MVP (egui/eframe)**

   * Device picker (LSL), live plots (downsampled), record button, rolling HR/HRV tiles, SQI heatbar; compile to Wasm w/ same code. ([GitHub][1])

---

# A few implementation tips

* **Columnar everywhere:** keep all time series in Arrow/Polars; blazing IPC to/from Python/R if users want to compare with NeuroKit/pyHRV. ([Docs.rs][3])
* **Deterministic numerics:** prefer `f64` for metrics; document resampling, windowing, detrending choices so results match reference tools.
* **Repro test harness:** bake in golden tests using PhysioNet RR series and known HRV outputs (± tiny tolerances). ([PhysioNet][19])
* **Streaming clocking:** if you use LSL, honor its timestamps and resync jitter before HRV calcs. ([GitHub][13])

---

[1]: https://github.com/emilk/egui?utm_source=chatgpt.com "egui: an easy-to-use immediate mode GUI in Rust that runs ..."
[2]: https://crates.io/crates/rustfft?utm_source=chatgpt.com "rustfft - crates.io: Rust Package Registry"
[3]: https://docs.rs/polars/latest/polars/?utm_source=chatgpt.com "polars - Rust"
[4]: https://github.com/labstreaminglayer/liblsl-rust?utm_source=chatgpt.com "labstreaminglayer/liblsl-rust: Rust wrapper for liblsl."
[5]: https://physionet.org/content/rr-interval-healthy-subjects/?utm_source=chatgpt.com "RR interval time series from healthy subjects v1.0.0"
[6]: https://github.com/PGomes92/opensignalsreader?utm_source=chatgpt.com "PGomes92/opensignalsreader"
[7]: https://www.physionet.org/content/wfdb/?utm_source=chatgpt.com "WFDB Software Package v10.7.0"
[8]: https://github.com/PIA-Group/BioSPPy?utm_source=chatgpt.com "GitHub - PIA-Group/BioSPPy: Biosignal Processing in Python"
[9]: https://github.com/neuropsychology/NeuroKit?utm_source=chatgpt.com "neuropsychology/NeuroKit: NeuroKit2: The Python Toolbox ..."
[10]: https://github.com/paulvangentcom/heartrate_analysis_python?utm_source=chatgpt.com "Python Heart Rate Analysis Package, for both PPG and ..."
[11]: https://github.com/PGomes92/pyhrv?utm_source=chatgpt.com "PGomes92/pyhrv: Python toolbox for Heart Rate Variability"
[12]: https://github.com/mne-tools/mne-python?utm_source=chatgpt.com "mne-tools/mne-python"
[13]: https://github.com/sccn/labstreaminglayer?utm_source=chatgpt.com "sccn/labstreaminglayer"
[14]: https://github.com/openbci-archive/OpenBCI_Python?utm_source=chatgpt.com "openbci-archive/OpenBCI_Python: The Python software ..."
[15]: https://github.com/phuselab/pyVHR?utm_source=chatgpt.com "phuselab/pyVHR: Python framework for Virtual Heart Rate"
[16]: https://link.springer.com/article/10.3758/s13428-020-01516-y?utm_source=chatgpt.com "NeuroKit2: A Python toolbox for neurophysiological signal ..."
[17]: https://neuropsychology.github.io/NeuroKit/functions/signal.html?utm_source=chatgpt.com "Signal Processing — NeuroKit2 0.2.13 documentation"
[18]: https://crates.io/crates/realfft?utm_source=chatgpt.com "realfft - crates.io: Rust Package Registry"
[19]: https://www.physionet.org/about/database/?utm_source=chatgpt.com "Databases"
