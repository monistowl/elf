# ELF — Extensible Lab Framework (bootstrap)

Local-first physiologic signal processing + biofeedback toolkit in Rust.

## Quickstart

```bash
# build all
cargo build --workspace

# run CLI
cat ecg.txt | cargo run -p elf-cli -- ecg-find-rpeaks --fs 250 | jq
cat rr.txt  | cargo run -p elf-cli -- hrv-time | jq

# run GUI
cargo run -p elf-gui
```

## Next steps
- Replace naive R-peak picker with proper pipeline (bandpass, diff, square, MWI, adaptive threshold).
- Add Welch PSD and nonlinear HRV in `elf-lib::metrics`.
- Add CSV/Parquet readers (enable `elf-lib` feature `polars`).
- Wire live streaming (LSL/OpenBCI adapters) and plots in `elf-gui`.
