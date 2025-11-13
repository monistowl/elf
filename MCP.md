Plan for integrating MCP (Model Context Protocol): https://github.com/modelcontextprotocol

Here’s a concrete plan you can hand to agents and keep maintainable.

# 1) What to build: `elf-mcp` sidecar

A small Rust daemon that speaks MCP over stdio/WebSocket and **wraps your existing crates**:

* Tools = idempotent, parameterized calls that mostly shell out to `elf-cli`, `elf-run`, etc., or call their library APIs directly.
* Resources = read-only handles to files in your **run bundles** (events.tsv, Parquet, logs), plus small JSON indices (catalogs).
* Prompts = canned “recipes” (e.g., design review, QA audit) that give agents scoped, high-signal context.

Keep it stateless; persist only **capability config** and a **catalog index**.

# 2) Tool surface (first wave)

Map each tool to a single, well-bounded action with explicit side-effect flags.

* `list_devices(lsl_filter?: string)` → discovered LSL streams + audio/video/trigger ports.
* `validate_design(design_toml: string, trials_csv: string)` → issues[], normalized design JSON.
* `simulate_run(design_toml: string, trials_csv: string, seed?: u64)` → returns a temp bundle id and path.
* `start_run(sub: string, ses: string, run: string, design_path: string, trials_path: string, devices: DeviceSpec, dry_run?: bool)` → run_id.
* `tail_events(run_id: string, since?: f64)` → streaming event rows (agent can “watch”).
* `list_bundles(query?: BundleQuery)` → metadata for completed runs; supports filters (sub/ses/task/date).
* `open_resource(uri: string)` → bytes/stream (e.g., `elf://bundle/<id>/events.tsv`).
* `derive_hrv(run_id: string, stream: string)` → computes HRV JSON/Parquet via `elf-cli`; registers as a new resource.
* `bundle_manifest(run_id: string)` → full manifest (versions, clocks, checksums).
* `signal_preview(run_id: string, stream: string, tmin?: f64, tmax?: f64, decimate?: u32)` → small downsampled slice for plotting.

Later:

* `design_suggest_counterbalance(...)`
* `qc_report(run_id)` (missed triggers, dropped frames, clock drifts)

# 3) Resources & URIs

Define a simple, stable scheme:

* `elf://bundle/<run_id>/events.tsv`
* `elf://bundle/<run_id>/<modality>/<file>`
* `elf://catalog/index.json` (searchable manifest)
* `elf://tmp/<id>/…` (simulation outputs)

Agents request resources by URI; `elf-mcp` streams bytes or slices (range support helps).

# 4) Prompts (agent recipes)

Ship a few high-leverage prompts that reference your MCP tools/resources:

* **“Design Reviewer”**: calls `validate_design`, then proposes counterbalancing/jitter fixes.
* **“Operator Copilot”**: steps: `list_devices` → `start_run` (dry) → `start_run` (live) → `tail_events`.
* **“QA Analyst”**: loads `bundle_manifest`, checks clocks/latencies, derives HRV, outputs a short audit.

These make agents useful on day 1 without bespoke code.

# 5) Implementation sketch (Rust)

* Use your library crates directly (prefer APIs over shelling out); fall back to `tokio::process::Command` where needed.
* Transport: stdio for local embedding; optional WebSocket (axum/tungstenite) if you’ll run it as a host service.
* Streaming: implement MCP’s **tool streaming** for `tail_events` and large resource reads.
* JSON schemas: reuse your `elf-schema` types; derive `serde` + `schemars` to publish tool parameter/response schemas.

Directory:

```
crates/
  elf-mcp/
    src/
      main.rs         # MCP server
      tools/*.rs      # tool handlers
      resources.rs    # URI router
      catalog.rs      # indexer
```

# 6) Safe side-effects & ops

* **Dry-run by default** for `start_run`; require `confirm: true` to arm.
* **Policy gates**: allow/deny tools at startup (e.g., disable hardware triggers in CI).
* **Sandbox**: runs write only under a configured `bundle_root`; resource URIs cannot escape it.
* **Auth** (if networked): token header or mutual TLS; log every tool call with arguments & caller.

# 7) Agent-friendly UX patterns

* All tools return **small JSON** + optional **resource URIs** for big payloads.
* Long jobs become **jobs**: `derive_hrv` returns `{job_id}` then progress ticks via streaming; final artifact is a resource URI.
* Determinism knobs everywhere (seed, hash of inputs) so agents can dedupe work.

# 8) ELF GUI integration (optional)

* Add an “Agent” panel that shows active runs/streams and lets an LLM control `elf-mcp` behind the scenes (operator approval dialog for side effects).
* For headless/servers, `elf-mcp` can run as a systemd service, and agents connect over WebSocket.

# 9) CI & packaging

* Build `elf-mcp` alongside other binaries.
* Ship a minimal **MCP manifest** (tools, schemas, resource patterns).
* Provide example agent configs (OpenAI, Anthropic, LangChain, etc.) pointing at `elf-mcp`.

# 10) Example flows

**Design → Simulate → Review**

1. Agent calls `validate_design`.
2. If ok, `simulate_run` (returns `elf://tmp/…/events.tsv`).
3. Agent reads resource, plots a preview (client side), suggests tweaks.

**Operator session**

1. `list_devices` → shows EEG, ECG, triggers.
2. `start_run(dry_run=true)` for timing check.
3. `start_run(dry_run=false)` to execute.
4. `tail_events` shows live stimuli/responses; on stop, agent fetches `bundle_manifest`.

**Post-hoc analysis**

1. `list_bundles` filter by `sub=01`.
2. `derive_hrv(run_id, "ECG")` → new resource URI.
3. Agent writes a short report referencing URIs.

# 11) Why this plays nicely with your stack

* Zero lock-in: MCP is a **thin adapter** over your CLI/lib; you can ditch it without touching core code.
* Reuse: both GUI and MCP invoke the **same `elf-run` engine**; no duplicated logic.
* Portability: works fully local; no crates.io; binaries ship with your installer.

