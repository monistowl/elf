#!/usr/bin/env bash
set -euo pipefail

# Sample pipeline that downloads a few MIT-BIH ECG records, runs ELF's HRV pipeline,
# and performs a simple permutation test comparing the mean RR interval between two records.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="$REPO_ROOT/test_data/physionet_experiment"
RECORD_DIR="$DATA_DIR/records"
mkdir -p "$RECORD_DIR"

BASE_URL="https://physionet.org/files/mitdb/1.0.0"
RECORDS=("100" "118" "205")

echo "Downloading MIT-BIH records to $RECORD_DIR"
for record in "${RECORDS[@]}"; do
  for ext in hea dat atr; do
    target="$RECORD_DIR/${record}.${ext}"
    if [ ! -f "$target" ]; then
      echo "  fetching $record.$ext"
      curl -fsSL "$BASE_URL/$record.$ext" -o "$target"
    fi
  done
done

elf_cmd() {
  cargo run -p elf-cli -- "$@"
}

echo "Running ELF HRV pipeline for records"
for record in "${RECORDS[@]}"; do
  output="$DATA_DIR/${record}.json"
  if [ ! -f "$output" ]; then
    echo "  processing record $record"
    elf_cmd beat-hrv-pipeline \
      --wfdb-header "$RECORD_DIR/${record}.hea" \
      --wfdb-lead 0 \
      > "$output"
  fi
done

PY_DATA_DIR="$DATA_DIR" python3 <<'PY'
import json
import pathlib
import random
import statistics

DATA_DIR = pathlib.Path(__import__("os").environ["PY_DATA_DIR"])
RECORDS = ["100", "118", "205"]
rr_storage = {}
hrv_storage = {}

for record in RECORDS:
    path = DATA_DIR / f"{record}.json"
    data = json.loads(path.read_text())
    rr_storage[record] = data["rr"]["rr"]
    hrv_storage[record] = data["hrv"]

print("HRV snapshots")
for record in RECORDS:
    hrv = hrv_storage[record]
    print(f"  Record {record}: RMSSD={hrv['rmssd']:.3f}s, SDNN={hrv['sdnn']:.3f}s, pNN50={hrv['pnn50']:.2%}")

group_a = rr_storage["100"]
group_b = rr_storage["205"]
obs_diff = statistics.mean(group_a) - statistics.mean(group_b)
combined = group_a + group_b
n1 = len(group_a)
n2 = len(group_b)
trials = 2000
count = 0
random.seed(0)
for _ in range(trials):
    permuted = combined[:]
    random.shuffle(permuted)
    mean_a = statistics.mean(permuted[:n1])
    mean_b = statistics.mean(permuted[n1:])
    if abs(mean_a - mean_b) >= abs(obs_diff):
        count += 1
p_value = (count + 1) / (trials + 1)

print("\nHypothesis test")
print(
    f"  Mean RR difference (100 - 205) = {obs_diff*1000:.1f} ms "
    f"(permutation p ≈ {p_value:.3f}, {trials} permutations)"
)
PY
