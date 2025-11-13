# v0.3 Roadmap ideas (generated after review — Nov 12, 2025)
1. **HRV analytics export + observability**
   * Add CLI/GUI `elf hrv-report` and `elf-gui` export that writes structured JSON/Parquet artifacts containing the full `Store` snapshot (waveform figure, RR histogram, PSD points, SQIs) plus provenance (config/seed). This enables external automation and ties the live dashboard back into dataset validation.
   * Bake in automatic comparison between live HRV metrics and historical runs (maybe via cached run bundles) so anomaly detection (e.g., sudden SQI drop) surfaces in both CLI logs and the GUI.
2. **Multi-device streaming orchestration**
   * Extend `StreamingStateRouter` to accept multiple LSL streams (ECG + eye) simultaneously and route each buffer to separate tab caches, so M3's focus on one stream per worker becomes a multiplexed orchestrator architecture.
   * Provide a `elf stream-status` watch command (CLI) or dashboard widget displaying connected stream names, channel counts, lag, and health.
3. **Scenario automation + replay**
   * Build `elf presenter` workflows that combine `scripts/generate_run_bundle.sh`, `elf run-simulate`, recorded video/pupil data, and metadata (ISI/jitter) into portable packages for deployment, plus CLI commands to replay them offline with instrumentation.
   * Integrate a `run-bundle` manifest viewer into the GUI (timeline + histograms + jitter) and allow exporting a signed CSV for posterity.
4. **Release-first infrastructure + QA**
   * Extend the release workflow to publish Windows/macOS/ARM builds (maybe via cross-compilation or multi-arch runners) and add a `just release` script that publishes multi-OS artifacts plus `scripts/install.sh` updates to the release notes.
   * Automatically validate the release via dataset suite + smoke tests (CLI commands + a headless GUI run) and attach the logs to the GH release.

Rationale: these ideas reuse the existing routers, dataset specs, and installer scripts while pushing toward more operational readiness for v0.3.
