# Caku Notes — November 12, 2025

## Streaming router parity
- `elf-gui` now ships an LSL inlet that resolves by `type`, applies clock sync/dejitter/monotonic options, and forwards samples to the background HRV worker and Parquet recorder.
- HRV controls expose a "Live LSL stream" block so Caku walkthroughs can instruct users to enter the stream type (e.g., `ECG`) and monitor connection metadata (name, source-id, channel count, nominal Hz).

## Recording status callouts
- The same panel now shows a "Parquet recording" section with Start/Stop buttons. Status strings are driven by the router (Idle/Starting/Recording/Error with sample counts) so presenters can confirm that captures are on disk before changing tasks.
- Recording prompts default to `recording.parquet` in the save dialog; installers should mention that the left rail turns red on errors so QA knows where to look.

## Packaging reminder
- Linux builds rely on the vendored `lsl-sys` + liblsl toolchain. Keep `__USE_DYNAMIC_STACK_SIZE=0` and `PTHREAD_STACK_MIN=16384` definitions when building release artifacts to avoid glibc 2.39+ compile failures.
