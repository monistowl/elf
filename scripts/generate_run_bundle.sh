#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
OUT_DIR="$REPO_ROOT/test_data/run_bundle"
mkdir -p "$OUT_DIR"
cargo run -p elf-cli -- run-simulate --design "${1:-test_data/run_design.toml}" \
  --trials "${2:-test_data/run_trials.csv}" --sub "${3:-01}" \
  --ses "${4:-01}" --run "${5:-01}" --out "$OUT_DIR"

python3 - <<PY
import csv, math
fs = 250.0
input_path = "$OUT_DIR/events.tsv"
out_path = "$OUT_DIR/events.idx"
with open(input_path) as src, open(out_path, 'w') as dst:
    reader = csv.reader(src, delimiter='\t')
    header = next(reader, None)
    for row in reader:
        if len(row) < 1:
            continue
        onset = float(row[0])
        idx = int(math.floor(onset * fs + 0.5))
        dst.write(f"{idx}\n")
PY
