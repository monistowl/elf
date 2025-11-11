#PHYSIO FILE FORMATS TO TEST ON

## A) Raw ECG with beat annotations (WFDB)

* **MIT-BIH Arrhythmia DB** — the classic: 48 × 30-min, 2-lead ambulatory ECG with expert beat labels (`.dat/.hea/.atr`). Perfect for validating R-peak detection end-to-end. ([PhysioNet][1])
* **INCART 12-lead Arrhythmia DB** — 75 records, 30-min each, 12-lead with reference beat annotations (notes on occasional misalignment). Great for multi-lead testing. ([PhysioNet][2])
* **PTB Diagnostic ECG DB** — 12-lead (plus Frank XYZ) resting ECGs at 1 kHz; high-resolution clinical variety. ([PhysioNet][3])
* **PTB-XL** — large 12-lead clinical set (10 s strips, rich labels; easy splits). Pairs nicely with **PTB-XL+** which adds **fiducial points/median beats** in WFDB/CSV—handy for “beat-detected” ground truth. ([PhysioNet][4])
* **European ST-T DB** — expert-annotated ischemia/ST-T changes; useful for non-beat features/segment QA. ([PhysioNet][5])

> WFDB annotation codes (`.atr`) and conventions are documented here; useful when you parse beats into `Events`. ([PhysioNet][6])

## B) RR-interval / “beat-detected” series (ready for `hrv-time`)

* **Normal Sinus Rhythm RR Interval DB (nsr2db)** — 54 long-term RR series with beat annotations; ideal for HRV metrics regression tests. ([PhysioNet][7])
* **RR interval time series from healthy subjects (2021)** — broad age range RR tachograms; clean CI/golden-file material. ([PhysioNet][8])

## C) CSV-friendly samples (good for quick plumbing tests)

* **BITalino / OpenSignals sample ECG** — downloadable TXT/CSV/HDF5 example recordings; mirrors what end-users often have. ([PLUX Support][9])
* **EDFbrowser** can convert EDF(+)/BDF(+) to CSV (and back) if you encounter those formats. ([Teunis van Beelen][10])

## D) Formats & conversion (if you want CSV pipes first)

* **WFDB “Format Conversions”** shows CSV↔WFDB workflows (`wfdb` Python). You can round-trip as needed for `elf-cli` pipes. ([WFDB Documentation][11])
* PhysioNet’s **“Creating WFDB-compatible records”** tutorial is the canonical overview of headers/signals/annotations. ([PhysioNet][12])

---

### How I’d use these with ELF

* **Validate detectors** on MIT-BIH & INCART (compare your `Events.indices` against `.atr` timestamps). ([PhysioNet][1])
* **Lock HRV math** with nsr2db + the 2021 healthy RR series (golden outputs for AVNN/SDNN/RMSSD/pNN50 and later PSD/Nonlinear). ([PhysioNet][7])
* **Smoke-test CSV ingestion** on BITalino/OpenSignals samples; they’re realistic lab files. ([GitHub][13])

[1]: https://www.physionet.org/physiobank/database/mitdb/?utm_source=chatgpt.com "MIT-BIH Arrhythmia Database v1.0.0"
[2]: https://physionet.org/content/incartdb/?utm_source=chatgpt.com "St Petersburg INCART 12-lead Arrhythmia Database v1.0.0"
[3]: https://www.physionet.org/physiobank/database/ptbdb/?utm_source=chatgpt.com "PTB Diagnostic ECG Database v1.0.0"
[4]: https://physionet.org/content/ptb-xl/?utm_source=chatgpt.com "PTB-XL, a large publicly available electrocardiography ..."
[5]: https://physionet.org/content/edb/?utm_source=chatgpt.com "European ST-T Database v1.0.0"
[6]: https://physionet.org/physiotools/wpg/wpg_36.htm?utm_source=chatgpt.com "WFDB Programmer's Guide: 4. Annotation Codes"
[7]: https://physionet.org/content/nsr2db/?utm_source=chatgpt.com "Normal Sinus Rhythm RR Interval Database v1.0.0"
[8]: https://physionet.org/content/rr-interval-healthy-subjects/?utm_source=chatgpt.com "RR interval time series from healthy subjects v1.0.0"
[9]: https://support.pluxbiosignals.com/knowledge-base/biosignalsplux-sensor-sample-signals-samples/?utm_source=chatgpt.com "biosignalsplux Sensor Sample Signals"
[10]: https://www.teuniz.net/edfbrowser/EDFbrowser%20manual.html?utm_source=chatgpt.com "EDFbrowser manual"
[11]: https://wfdb.readthedocs.io/en/latest/convert.html?utm_source=chatgpt.com "Format Conversions — wfdb \"4.3.0\" documentation"
[12]: https://archive.physionet.org/tutorials/creating-records.shtml?utm_source=chatgpt.com "Creating PhysioBank (WFDB-compatible) Records and ..."
[13]: https://github.com/BITalinoWorld/revolution-sample-data?utm_source=chatgpt.com "Sample sensor data acquired using BITalino (r)evolution ..."

