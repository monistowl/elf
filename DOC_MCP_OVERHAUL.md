Plan for auto-documenting with MCP.
Easiest path: **generate JSON Schemas from your Rust types and publish them (plus Markdown) as MCP resources**. You don’t need a web server or crates.io—just `schemars` + `serde` and a tiny renderer.

## Minimal stack

* **`serde`** for request/response types
* **`schemars`** (`JsonSchema` derive) to auto-generate parameter/result schemas
* **`clap`** (you already use it) to harvest `--help` text for CLI tools
* *(optional)* **`utoipa`** if you want OpenAPI + ReDoc/Swagger immediately
* *(optional)* **`comrak`** to render Markdown from templates if you want pretty local docs

## How it fits MCP

Your `elf-mcp` sidecar can expose two things:

1. **A tool**: `list_tools()` → returns the tool registry (names, summaries, input/output schemas).
2. **Resources**:

   * `elf://docs/tools.md` — Markdown overview
   * `elf://docs/schemas/<tool>.schema.json` — per-tool JSON Schema
   * `elf://docs/openapi.json` — optional OpenAPI doc

Agents can read these resources to self-discover capabilities.

## Sketch: registry + schema export

````rust
// Cargo.toml
// schemars = "0.8"
// serde = { version = "1", features = ["derive"] }

use schemars::{schema::RootSchema, schema_for};
use serde::{Deserialize, Serialize};

pub trait McpTool {
    type Params: Serialize + for<'de> Deserialize<'de> + schemars::JsonSchema;
    type Result: Serialize + for<'de> Deserialize<'de> + schemars::JsonSchema;

    fn name(&self) -> &'static str;
    fn summary(&self) -> &'static str;
}

pub struct ToolSpec {
    pub name: &'static str,
    pub summary: &'static str,
    pub params_schema: RootSchema,
    pub result_schema: RootSchema,
}

pub fn spec_from<T: McpTool>(tool: &T) -> ToolSpec {
    ToolSpec {
        name: tool.name(),
        summary: tool.summary(),
        params_schema: schema_for!(T::Params),
        result_schema:  schema_for!(T::Result),
    }
}

pub fn render_markdown(tools: &[ToolSpec]) -> String {
    let mut s = String::from("# ELF MCP Tools\n\n");
    for t in tools {
        use serde_json::to_string_pretty;
        s.push_str(&format!("## {}\n{}\n\n", t.name, t.summary));
        s.push_str("**Params schema**:\n\n```json\n");
        s.push_str(&to_string_pretty(&t.params_schema).unwrap());
        s.push_str("\n```\n\n**Result schema**:\n\n```json\n");
        s.push_str(&to_string_pretty(&t.result_schema).unwrap());
        s.push_str("\n```\n\n");
    }
    s
}
````

### Example tool

```rust
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidateDesignParams { pub design_toml: String, pub trials_csv: String }

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValidateDesignResult { pub ok: bool, pub issues: Vec<String> }

pub struct ValidateDesign;

impl McpTool for ValidateDesign {
    type Params = ValidateDesignParams;
    type Result = ValidateDesignResult;
    fn name(&self) -> &'static str { "validate_design" }
    fn summary(&self) -> &'static str { "Validate a TOML design + CSV trials." }
}
```

### Wiring into `elf-mcp`

* Keep a `Vec<ToolSpec>` built at startup.
* Implement MCP methods/resources:

  * `list_tools` → serialize that `Vec<ToolSpec>` (but strip big schemas or paginate)
  * `open_resource("elf://docs/tools.md")` → return the Markdown string
  * `open_resource("elf://docs/schemas/<name>.schema.json")` → return `params_schema`/`result_schema`

## Fold in CLI docs (from `clap`)

You can grab each binary’s help text to embed alongside schemas:

```rust
use clap::CommandFactory;
use elf_cli::Cli; // your root Clap struct
let mut cmd = Cli::command();
let help = cmd.render_long_help().to_string();
```

Add `help` to your `ToolSpec` or a separate resource `elf://docs/cli/<bin>.md`.

## Optional: one-file OpenAPI with `utoipa`

If you prefer a prebuilt doc site:

* Derive `ToSchema` (from `utoipa`) on your types (or bridge from `schemars`).
* Build an OpenAPI doc (even without HTTP routes), dump to `openapi.json` in `elf-mcp`.
* Serve a static **ReDoc** HTML (bundled) as `elf://docs/index.html` that points to `openapi.json`.

This gives you instant, navigable docs that agents can fetch or you can open in a browser—without publishing crates.

## Developer flow (no crates.io)

* `cargo xtask gen-docs`:

  * Builds the tool registry
  * Writes `docs/tools.md`, `docs/schemas/*.json`, and optional `docs/openapi.json`
* `elf-mcp` loads the same registry and exposes the docs as resources.

## Why this is “easy”

* You **reuse your real types**; schemas stay in sync.
* No server is required; MCP resources are just bytes.
* Works offline; perfect for local-first installs.

