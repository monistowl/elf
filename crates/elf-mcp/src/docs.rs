use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Brief descriptor for each MCP tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDoc {
    pub name: &'static str,
    pub summary: &'static str,
    pub params_schema: Value,
    pub result_schema: Value,
    pub help: &'static str,
}

impl ToolDoc {
    pub fn new(
        name: &'static str,
        summary: &'static str,
        params_schema: Value,
        result_schema: Value,
        help: &'static str,
    ) -> Self {
        Self {
            name,
            summary,
            params_schema,
            result_schema,
            help,
        }
    }
}

/// Registry of tool documentation / schemas.
#[derive(Debug, Clone)]
pub struct DocRegistry {
    tools: Vec<ToolDoc>,
}

impl DocRegistry {
    pub fn new() -> Self {
        let bundle_schema = json!({
            "type": "object",
            "properties": {
                "run_id": { "type": "string" },
                "subject": { "type": "string" },
                "session": { "type": "string" },
                "task": { "type": "string" },
                "design": { "type": "string" },
                "total_trials": { "type": "integer", "minimum": 0 },
                "total_events": { "type": "integer", "minimum": 0 },
                "seed": { "type": ["integer", "null"] },
                "randomization_policy": { "type": ["string", "null"] },
                "isi_ms": { "type": "number", "minimum": 0 },
                "isi_jitter_ms": { "type": ["number", "null"], "minimum": 0 },
                "started": { "type": "number" },
                "bundle_path": { "type": "string" }
            },
            "required": ["run_id", "subject", "session", "task", "design", "total_trials", "total_events", "isi_ms", "started", "bundle_path"],
            "additionalProperties": false
        });

        let manifest_schema = json!({
            "type": "object",
            "properties": {
                "sub": { "type": "string" },
                "ses": { "type": "string" },
                "run": { "type": "string" },
                "task": { "type": "string" },
                "design": { "type": "string" },
                "total_trials": { "type": "integer", "minimum": 0 },
                "total_events": { "type": "integer", "minimum": 0 },
                "seed": { "type": ["integer", "null"] },
                "randomization_policy": { "type": ["string", "null"] },
                "isi_ms": { "type": "number", "minimum": 0 },
                "isi_jitter_ms": { "type": ["number", "null"], "minimum": 0 },
                "start_time_unix": { "type": "number" }
            },
            "required": ["sub", "ses", "run", "task", "design", "total_trials", "total_events", "isi_ms", "start_time_unix"],
            "additionalProperties": false
        });

        let tools = vec![
            ToolDoc::new(
                "catalog_index",
                "Read the in-memory catalog summary (bundles + metadata).",
                json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                json!({
                    "type": "object",
                    "properties": {
                        "bundles": { "type": "array", "items": bundle_schema },
                        "count": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["bundles", "count"],
                    "additionalProperties": false
                }),
                "Use this to build registry/index caches before calling other tools.",
            ),
            ToolDoc::new(
                "list_bundles",
                "Return the list of known bundles discovered from disk.",
                json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                json!({ "type": "array", "items": bundle_schema }),
                "This is a faceted view of the catalog entries that agents can page through.",
            ),
            ToolDoc::new(
                "bundle_manifest",
                "Fetch the manifest for a single run ID.",
                json!({
                    "type": "object",
                    "properties": { "run": { "type": "string" } },
                    "required": ["run"],
                    "additionalProperties": false
                }),
                manifest_schema.clone(),
                "Requires the `run` parameter (e.g., sub-ses-run).",
            ),
            ToolDoc::new(
                "open_resource",
                "Read any available resource URI (bundles, docs, etc.).",
                json!({
                    "type": "object",
                    "properties": { "uri": { "type": "string" } },
                    "required": ["uri"],
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "uri": { "type": "string" },
                        "bytes": { "type": "integer", "minimum": 0 },
                        "base64": { "type": "string" }
                    },
                    "required": ["uri", "bytes", "base64"],
                    "additionalProperties": false
                }),
                "Agents should use this for streaming data via elf:// URIs.",
            ),
            ToolDoc::new(
                "simulate_run",
                "Simulate a stimulus run from design + trial specs.",
                json!({
                    "type": "object",
                    "properties": {
                        "design": { "type": "string" },
                        "trials": { "type": "string" },
                        "sub": { "type": "string" },
                        "ses": { "type": "string" },
                        "run": { "type": "string" }
                    },
                    "required": ["design", "trials"],
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "bundle_id": { "type": "string" },
                        "sub": { "type": "string" },
                        "ses": { "type": "string" },
                        "task": { "type": "string" },
                        "design": { "type": "string" },
                        "resources": {
                            "type": "object",
                            "properties": {
                                "events": { "type": "string" },
                                "manifest": { "type": "string" },
                                "metadata": { "type": "string" }
                            },
                            "required": ["events", "manifest"]
                        },
                        "directory": { "type": "string" },
                        "tmp_id": { "type": "string" }
                    },
                    "required": ["bundle_id", "resources", "tmp_id", "directory"],
                    "additionalProperties": false
                }),
                "Wraps elf-cli's `run-simulate` functionality via elf_run.",
            ),
            ToolDoc::new(
                "list_devices",
                "List connected acquisition hardware (LSL, audio triggers, etc.).",
                json!({
                    "type": "object",
                    "properties": {
                        "filter": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "devices": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "name": { "type": "string" },
                                    "type": { "type": "string" },
                                    "channels": { "type": "integer", "minimum": 1 },
                                    "sampling_rate": { "type": "number" },
                                    "description": { "type": "string" }
                                },
                                "required": ["id", "name", "type", "channels"]
                            }
                        }
                    },
                    "required": ["devices"],
                    "additionalProperties": false
                }),
                "`list_devices(filter=...)` lets operators inspect available streams.",
            ),
            ToolDoc::new(
                "start_run",
                "Arm (or dry-run) a stimulus session and produce bundle URIs.",
                json!({
                    "type": "object",
                    "properties": {
                        "design": { "type": "string" },
                        "trials": { "type": "string" },
                        "sub": { "type": "string" },
                        "ses": { "type": "string" },
                        "run": { "type": "string" },
                        "dry_run": { "type": "boolean" },
                        "confirm": { "type": "boolean" },
                        "devices": { "type": "array" }
                    },
                    "required": ["design", "trials"],
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "bundle_id": { "type": "string" },
                        "tmp_id": { "type": "string" },
                        "resources": { "type": "object" },
                        "status": { "type": "string" },
                        "dry_run": { "type": "boolean" },
                        "mode": { "type": "string" }
                    },
                    "required": ["bundle_id", "resources", "tmp_id"],
                    "additionalProperties": false
                }),
                "`start_run` is the live instrument entry point; confirm=true launches it.",
            ),
            ToolDoc::new(
                "tail_events",
                "Stream recent events for a run (supports since/limit).",
                json!({
                    "type": "object",
                    "properties": {
                        "run": { "type": "string" },
                        "tmp_id": { "type": "string" },
                        "since": { "type": "number" },
                        "limit": { "type": "integer" }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "source": { "type": "string" },
                        "count": { "type": "integer" },
                        "events": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "onset": { "type": "number" },
                                    "duration": { "type": "number" },
                                    "event_type": { "type": "string" },
                                    "stim_id": { "type": "string" }
                                },
                                "required": ["onset", "event_type", "stim_id"]
                            }
                        }
                    },
                    "required": ["source", "events"],
                    "additionalProperties": false
                }),
                "Great for operator dashboards that watch _stim_ events on-the-fly.",
            ),
            ToolDoc::new(
                "derive_hrv",
                "Compute HRV metrics (time/frequency/nonlinear) from bundle events.",
                json!({
                    "type": "object",
                    "properties": {
                        "run": { "type": "string" },
                        "tmp_id": { "type": "string" },
                        "stream": { "type": "string" }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "stream": { "type": "string" },
                        "source": { "type": "string" },
                        "rr_series": {
                            "type": "array",
                            "items": { "type": "number" }
                        },
                        "hrv_time": { "type": "object" },
                        "hrv_psd": { "type": "object" },
                        "hrv_nonlinear": { "type": "object" }
                    },
                    "required": ["stream", "source", "rr_series", "hrv_time", "hrv_psd", "hrv_nonlinear"],
                    "additionalProperties": false
                }),
                "Calls the shared HRV metrics (time/frequency/nonlinear) over stim timing.",
            ),
            ToolDoc::new(
                "signal_preview",
                "Fetch a downsampled slice of events for plotting.",
                json!({
                    "type": "object",
                    "properties": {
                        "run": { "type": "string" },
                        "tmp_id": { "type": "string" },
                        "stream": { "type": "string" },
                        "tmin": { "type": "number" },
                        "tmax": { "type": "number" },
                        "decimate": { "type": "integer" }
                    },
                    "additionalProperties": false
                }),
                json!({
                    "type": "object",
                    "properties": {
                        "stream": { "type": "string" },
                        "source": { "type": "string" },
                        "tmin": { "type": "number" },
                        "tmax": { "type": "number" },
                        "decimate": { "type": "integer" },
                        "events": {
                            "type": "array",
                            "items": { "type": "object" }
                        },
                        "annotations": { "type": ["array", "null"] }
                    },
                    "required": ["stream", "events", "source"],
                    "additionalProperties": false
                }),
                "Useful for rendering quick signal plots or verifying device data.",
            ),
            ToolDoc::new(
                "list_tools",
                "Describe every registered MCP tool (with schemas).",
                json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "summary": { "type": "string" },
                            "params_schema": { "type": "object" },
                            "result_schema": { "type": "object" },
                            "help": { "type": "string" }
                        },
                        "required": ["name", "summary", "params_schema", "result_schema", "help"],
                        "additionalProperties": false
                    }
                }),
                "Useful for discovery and documentation generation.",
            ),
        ];

        Self { tools }
    }

    pub fn list(&self) -> Vec<ToolDoc> {
        self.tools.clone()
    }

    pub fn render_markdown(&self) -> String {
        let mut buffer = String::from("# ELF MCP Tools\n\n");
        for doc in &self.tools {
            buffer.push_str(&format!("## `{}`\n{}\n\n", doc.name, doc.summary));
            buffer.push_str("**CLI help**\n\n```\n");
            buffer.push_str(doc.help);
            buffer.push_str("\n```\n\n");
            buffer.push_str("**Parameters schema**\n\n```json\n");
            buffer.push_str(&serde_json::to_string_pretty(&doc.params_schema).unwrap());
            buffer.push_str("\n```\n\n**Result schema**\n\n```json\n");
            buffer.push_str(&serde_json::to_string_pretty(&doc.result_schema).unwrap());
            buffer.push_str("\n```\n\n");
        }
        buffer
    }

    pub fn schema_payload(&self, name: &str) -> Option<Value> {
        self.tools.iter().find(|doc| doc.name == name).map(|doc| {
            json!({
                "name": doc.name,
                "summary": doc.summary,
                "params_schema": doc.params_schema,
                "result_schema": doc.result_schema,
                "help": doc.help,
            })
        })
    }

    pub fn openapi(&self) -> Value {
        let mut paths = serde_json::Map::new();
        for doc in &self.tools {
            paths.insert(
                format!("/tools/{}", doc.name),
                json!({
                    "post": {
                        "summary": doc.summary,
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": doc.params_schema
                                }
                            }
                        },
                        "responses": {
                            "200": {
                                "description": "Tool result",
                                "content": {
                                    "application/json": {
                                        "schema": doc.result_schema
                                    }
                                }
                            }
                        }
                    }
                }),
            );
        }

        json!({
            "openapi": "3.0.0",
            "info": {
                "title": "ELF MCP Tools",
                "version": "0.1.0",
                "description": "Auto-generated reference for elf-mcp endpoints."
            },
            "paths": paths
        })
    }

    pub fn resource_bytes(&self, path: &str) -> Option<Vec<u8>> {
        match path {
            "tools.md" => Some(self.render_markdown().into_bytes()),
            "openapi.json" => serde_json::to_vec_pretty(&self.openapi()).ok(),
            other if other.starts_with("schemas/") => {
                let tool = &other["schemas/".len()..];
                self.schema_payload(tool)
                    .and_then(|schema| serde_json::to_vec_pretty(&schema).ok())
            }
            _ => None,
        }
    }

    pub fn resolve_uri(&self, uri: &str) -> Option<Vec<u8>> {
        const PREFIX: &str = "elf://docs/";
        uri.strip_prefix(PREFIX)
            .and_then(|path| self.resource_bytes(path))
    }
}

impl Default for DocRegistry {
    fn default() -> Self {
        Self::new()
    }
}
