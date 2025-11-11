Notes on implementing EEG tools/formats:
# Seizure / clinical EEG

* **CHB-MIT Scalp EEG** (pediatric, long recordings, seizure onsets/offsets). Formats: EDF-like/WFDB on PhysioNet; explicit seizure annotation files. Great for event parsing + long-form streaming tests. ([PhysioNet][1])
* **TUH EEG / TUH Seizure (TUSZ)** (very large clinical corpus; subsets with manual seizure labels, channels, types). Note: free but requires signup/accept terms. Good for scale, diverse montages, and rich label taxonomy. ([isip.piconepress.com][2])

# Sleep EEG (stage labels, arousals)

* **Sleep-EDF (Expanded)** (PSG with EEG/EOG/EMG + technician-scored hypnograms). Format: EDF with separate annotations; perfect for epoching pipelines and label alignment. ([PhysioNet][3])

# Motor imagery / BCI (trial events, clean paradigms)

* **BCI Competition IV (2a, etc.)** (22-ch EEG, 4-class MI; per-trial labels). Often distributed as GDF/Mat; many community CSV mirrors exist for quick trials. Great for event-locked analysis. ([Berlin Brain-Computer Interface][4])
* **PhysioNet: EEG Motor Movement/Imagery** (109 volunteers; event codes for motor tasks). Good general MI benchmark with standard annotations. ([PhysioNet][5])

# BIDS-native EEG (clean event files, easy metadata)

* **OpenNeuro** (many EEG/iEEG sets; all BIDS-validated with `events.tsv`). Search by “EEG”; e.g., ds002718 (face processing). Excellent for testing your **BIDS reader** and tabbed dashboards. ([OpenNeuro][6])
* **CHB-MIT (BIDS-converted mirror)**—community repackage; handy if you want CHB-MIT with BIDS conventions out of the box. ([Zenodo][7])

---

## Formats you’ll encounter (and what to support early)

* **EDF/EDF+ / BDF** (Sleep-EDF, many clinical sets).
* **WFDB + separate annotation files** (PhysioNet’s style for some sets).
* **BIDS** (directory layout + `events.tsv` per run; the easiest long-term). OpenNeuro datasets must validate against BIDS, which keeps interop pain low. ([MNE Tools][8])

---

## How I’d plug these into ELF

1. **Readers**: start with **EDF(+)/BDF** and **BIDS events.tsv**; add a small **WFDB annotation** parser for CHB-MIT.
2. **Gold tests**:

   * Event alignment: seizure onset/offset (CHB-MIT, TUH). ([PhysioNet][1])
   * Epoching + labels: Sleep-EDF hypnograms; MI trial events. ([PhysioNet][3])
3. **GUI**: BIDS makes it trivial to populate a “Runs / Channels / Events” sidebar and plot event-locked averages.


[1]: https://physionet.org/content/chbmit/1.0.0/?utm_source=chatgpt.com "CHB-MIT Scalp EEG Database v1.0.0"
[2]: https://isip.piconepress.com/projects/nedc/html/tuh_eeg/?utm_source=chatgpt.com "Temple University EEG Corpus - Downloads"
[3]: https://www.physionet.org/content/sleep-edfx/1.0.0/?utm_source=chatgpt.com "Sleep-EDF Database Expanded v1.0.0"
[4]: https://www.bbci.de/competition/iv/?utm_source=chatgpt.com "BCI Competition IV"
[5]: https://www.physionet.org/about/database/?utm_source=chatgpt.com "Databases"
[6]: https://openneuro.org/?utm_source=chatgpt.com "OpenNeuro"
[7]: https://zenodo.org/records/10259996?utm_source=chatgpt.com "BIDS CHB-MIT Scalp EEG Database"
[8]: https://mne.tools/mne-bids/stable/auto_examples/read_bids_datasets.html?utm_source=chatgpt.com "Read BIDS datasets"
