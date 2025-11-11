
# Where to get data

## Open, research-grade (often BIDS or close)

* **OpenNeuro** — search for datasets with eye-tracking/pupillometry. Two good starters:

  * *ds003673* (resting-state fMRI + simultaneous pupillometry): calibrated pupil traces with scanner-synced timing. Great for long, steady baselines. ([OpenNeuro][1])
  * *ds003838* (EEG + ECG/PPG + pupillometry, 86 participants): perfect for cross-modal tests and timestamp sanity checks. ([OpenNeuro][2])
  * (Also: naturalistic movie sets with calibrated gaze & pupil, e.g., *ds006642*.) ([OpenNeuro][3])

* **BIDS status for eye-tracking** — BIDS supports extensions via BEPs; dedicated eye-tracking specs are in the works/announced in BIDS channels and INCF deliverables. If you aim for future-proof layout, mirror BIDS (subject/session/run folders + `events.tsv`). ([BIDS Specification][4])

## Vendor ecosystems (free sample recordings, easy CSV/TSV)

* **Pupil Labs (Core/Invisible/Neon)** — docs include **sample recordings** you can download from Player/Cloud; exports contain per-sample pupil size, confidence, and events in CSV/JSON alongside eye/world video. Ideal for end-to-end “real hardware” trials without owning the hardware. ([docs.pupil-labs.com][5])
* **Tobii Pro** — Pro Lab exports TSV with **pupil diameters in millimeters**, sampling rate, and lots of metadata; Tobii’s docs spell out pupil fields clearly. Great for CSV/TSV ingestion and unit handling. ([connect.tobii.com][6])
* **SR Research EyeLink** — raw **.edf** (proprietary eye-tracker EDF, not the EEG/physio EDF) converts to ASCII with **edf2asc** (CLI/GUI). Once ASCII, you get per-sample pupil plus message/event streams—perfect for your `events.tsv` builder. ([SR Research][7])

## Open hardware / open source

* **PupilEXT** (open-source pupillometry platform) — not a dataset per se, but great for generating reproducible, annotated recordings or testing algorithms on external videos. Paper + software are both open. ([Frontiers][8])

# Common formats you’ll see (and how to handle)

* **BIDS-like**: signal in TSV/CSV (`pupil_left/right`, `confidence`, `timestamp`), events in `events.tsv`. Easy to parse and align.
* **Pupil Labs**: folder per recording with CSV/JSON “exports” (and optionally video). Use timestamps and confidence; ignore low-confidence samples.
* **Tobii Pro**: TSV with mm-scale pupil diameters; watch for per-eye columns, missing data markers, and device-specific sampling.
* **EyeLink**: convert `.edf` → `.asc` via edf2asc; parse “SAMPLES” blocks (pupil) and “MSG” lines (events). ([SR Research][7])

# What to test in ELF (quick checklist)

1. **Ingestion**

   * Add readers for **Pupil Labs CSV**, **Tobii TSV**, **EyeLink ASC**, and **BIDS events.tsv**.
   * Normalize to a common struct: `{ t: f64 (s), pupil_mm: f32, eye: Left/Right/Binocular, conf: f32 }`.

2. **Preprocessing**

   * **Blink detection + interpolation** (NaNs or conf<thr → gap fill with shape-preserving interpolation).
   * **Dilation baseline/band-limit** (HP ~0.01–0.05 Hz + LP ~4 Hz as a starting envelope).
   * **Downsampling** for plots to ≤ ~2k points per line to keep egui smooth.

3. **Events & epochs**

   * From BIDS `events.tsv` or EyeLink MSG, epoch −1…+3 s around events; compute mean/peak dilation, latency.
   * Validate on OpenNeuro MI/sleep/cognition sets that include pupillometry (e.g., ds003838). ([OpenNeuro][2])

4. **Units & geometry**

   * Prefer **millimeters** for pupil size when available (Tobii already exports mm). If only pixel units exist, keep the scale metadata; only convert if a calibration factor is provided. ([connect.tobii.com][9])

# Handy reference tooling (for parity/testing)

* **PuPl** (open-source pupillometry pipeline) — a good baseline for blink correction and epoching behavior to match in tests. ([SpringerLink][10])
* **Method tutorials** — time-course analysis tutorials give canonical preprocessing steps and stats models; useful to sanity-check your pipeline defaults. ([PMC][11])

# Suggested “first adapters” for `elf-lib`

* `pupil_labs::read_export(path) -> PupilSeries + Events`
* `tobii::read_tsv(path) -> PupilSeries + Events`
* `eyelink::asc::read(path) -> PupilSeries + Events` (document edf2asc requirement) ([SR Research][7])
* `bids::read_eye(path) -> PupilSeries + Events` (scan `<sub>/<ses>/eyetrack/*_eyetrack.tsv` + `events.tsv`)

Then wire quick `elf-cli` helpers:

```bash
# Convert vendor export → normalized CSV
elf-cli pupil-normalize --in sample.tsv --format tobii --out pupil.csv

# Blink-clean + baseline-correct
cat pupil.csv | elf-cli pupil-clean --hp 0.02 --lp 4.0 > pupil_clean.csv

# Event-locked averages (BIDS)
elf-cli pupil-epa --pupil pupil_clean.csv --events events.tsv --tmin -1 --tmax 3 > erpdil.json
```


[1]: https://openneuro.org/datasets/ds003673?utm_source=chatgpt.com "Yale Resting State fMRI/Pupillometry: Arousal Study"
[2]: https://openneuro.org/datasets/ds003838?utm_source=chatgpt.com "EEG, pupillometry, ECG and photoplethysmography, and ..."
[3]: https://openneuro.org/datasets/ds006642?utm_source=chatgpt.com "Naturalistic Neuroimaging Database 3T+"
[4]: https://bids-specification.readthedocs.io/en/v1.2.1/06-extensions.html?utm_source=chatgpt.com "Extending the BIDS specification - Brain Imaging Data ..."
[5]: https://docs.pupil-labs.com/core/software/pupil-player/?utm_source=chatgpt.com "Core - Pupil Player"
[6]: https://connect.tobii.com/s/article/Pupil-related-data-in-Tobii-Pro-Lab?utm_source=chatgpt.com "Pupil-related data in Tobii Pro Lab"
[7]: https://www.sr-research.com/support/thread-7674.html?utm_source=chatgpt.com "EDF to ASCII Conversion / EDF2ASC"
[8]: https://www.frontiersin.org/journals/neuroscience/articles/10.3389/fnins.2021.676220/full?utm_source=chatgpt.com "PupilEXT: Flexible Open-Source Platform for High- ..."
[9]: https://connect.tobii.com/s/article/measuring-pupil-size?utm_source=chatgpt.com "Measuring pupil size"
[10]: https://link.springer.com/article/10.3758/s13428-021-01717-z?utm_source=chatgpt.com "PuPl: an open-source tool for processing pupillometry data"
[11]: https://pmc.ncbi.nlm.nih.gov/articles/PMC6535748/?utm_source=chatgpt.com "Analyzing the Time Course of Pupillometric Data - PMC"
