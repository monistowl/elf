# Presenter Bundles (run-simulate)

- `elf run-simulate` reads the TOML design + CSV trials, applies jitter/randomization (block shuffle or permute), and writes `events.tsv` + `events.json` metadata + `run.json` manifest containing the sampling metadata (`isi_ms`, `isi_jitter_ms`, `randomization_policy`, `seed`).
- The GUI `Load run bundle` button now replays the same stats by converting the TSV onsets into `Events`, feeding them back through the shared `Store` (via `submit_events`) so PSD/HRV plots stay consistent with the CLI manifest, and surfacing the manifest info (task, ISI + jitter, randomization policy) alongside the HRV controls.
- Installer notes/docs should mention that presenters can package their TOML/CSV specs via `run-simulate` before shipping the `events.tsv`/`run.json` bundle, ensuring labs run the same stimuli & HRV snapshots.
