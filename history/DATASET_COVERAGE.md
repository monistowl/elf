# Dataset coverage update — November 2025

- Added OpenBCI CSV support inside `elf-cli dataset-validate`, reusing the same `openbci_io::read_openbci_csv` helper as the `open-bci` CLI command so the validation suite can check that adapter as well.
- Extended `test_data/dataset_suite_core.json` with a `openbci_ch1` case that points at the `test_data/openbci_sample.csv` fixture and asserts the deterministic HRV time/PSD metrics (all zeros for this short synthetic segment) so the suite now catches regressions when touching OpenBCI ingestion.
- New history note keeps an audit trail, matching our policy of recording AI-generated planning docs under `history/` and never staging them.

- Added `run_bundle_stim` to the dataset suite: a newline list of run-simulate stimulus onsets lives in `test_data/run_bundle/events.idx`, and dataset-validate now ensures the GUI/CLI replay the same HRV/RRPS specs that presenters expect.
