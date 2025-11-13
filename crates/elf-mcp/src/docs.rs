use crate::doc_types::*;
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::OpenApi;

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

    pub fn to_description(&self) -> ToolDescription {
        ToolDescription {
            name: self.name.to_string(),
            summary: self.summary.to_string(),
            params_schema: self.params_schema.clone(),
            result_schema: self.result_schema.clone(),
            help: self.help.to_string(),
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
        let tools = build_tool_docs();
        Self { tools }
    }

    pub fn list(&self) -> ListToolsResult {
        self.tools.iter().map(|doc| doc.to_description()).collect()
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
                "params_schema": doc.params_schema.clone(),
                "result_schema": doc.result_schema.clone(),
                "help": doc.help,
            })
        })
    }

    pub fn openapi(&self) -> Value {
        let json = McpOpenApi::openapi().to_json().expect("serialize openapi");
        serde_json::from_str(&json).expect("parse openapi")
    }

    pub fn resource_bytes(&self, path: &str) -> Option<Vec<u8>> {
        match path {
            "tools.md" => Some(self.render_markdown().into_bytes()),
            "openapi.json" => serde_json::to_vec_pretty(&self.openapi()).ok(),
            "index.html" => Some(REDOC_HTML.as_bytes().to_vec()),
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

fn build_tool_docs() -> Vec<ToolDoc> {
    vec![
        doc_entry::<CatalogIndexParams, CatalogIndexResult>(
            "catalog_index",
            "Read the in-memory catalog summary (bundles + metadata).",
            "Use this to build registry/index caches before calling other tools.",
        ),
        doc_entry::<ListBundlesParams, ListBundlesResult>(
            "list_bundles",
            "Return the list of known bundles discovered from disk.",
            "This is a faceted view of the catalog entries that agents can page through.",
        ),
        doc_entry::<BundleManifestParams, RunManifestDoc>(
            "bundle_manifest",
            "Fetch the manifest for a single run ID.",
            "Requires the `run` parameter (e.g., sub-ses-run).",
        ),
        doc_entry::<OpenResourceParams, OpenResourceResult>(
            "open_resource",
            "Read any available resource URI (bundles, docs, catalog).",
            "Fetch files or generated docs by their elf:// resource identifiers.",
        ),
        doc_entry::<ListDevicesParams, DeviceCatalog>(
            "list_devices",
            "Describe every device/stream configured on this rig.",
            "`list_devices(filter=...)` lets operators inspect available streams.",
        ),
        doc_entry::<SimulateRunParams, SimulateRunResult>(
            "simulate_run",
            "Dry-run a stimulus session and materialize bundle resources in /tmp.",
            "Feed design/trials to generate bundle URIs for downstream inspection.",
        ),
        doc_entry::<StartRunParams, StartRunResult>(
            "start_run",
            "Arm (or dry-run) a stimulus session and produce bundle URIs.",
            "Call with `confirm=true` when `dry_run=false` to go live.",
        ),
        doc_entry::<TailEventsParams, TailEventsResult>(
            "tail_events",
            "Stream recent events for a run (supports since/limit).",
            "Great for operator dashboards that watch stim events on-the-fly.",
        ),
        doc_entry::<DeriveHrvParams, DeriveHrvResult>(
            "derive_hrv",
            "Compute HRV metrics (time/frequency/nonlinear) from bundle events.",
            "Calls the shared HRV metrics over stim timing.",
        ),
        doc_entry::<SignalPreviewParams, SignalPreviewResult>(
            "signal_preview",
            "Fetch a downsampled slice of events for plotting.",
            "Useful for rendering quick signal plots or verifying device data.",
        ),
        doc_entry::<ListToolsParams, ListToolsResult>(
            "list_tools",
            "Describe every registered MCP tool (with schemas).",
            "Useful for discovery and documentation generation.",
        ),
    ]
}

fn doc_entry<P, R>(name: &'static str, summary: &'static str, help: &'static str) -> ToolDoc
where
    P: schemars::JsonSchema,
    R: schemars::JsonSchema,
{
    let params_schema = serde_json::to_value(schema_for!(P)).unwrap_or_else(|_| json!({}));
    let result_schema = serde_json::to_value(schema_for!(R)).unwrap_or_else(|_| json!({}));
    ToolDoc::new(name, summary, params_schema, result_schema, help)
}

const REDOC_HTML: &str = r#"<!DOCTYPE html>
<html lang=\"en\">
<head>
<meta charset=\"utf-8\" />
<title>ELF MCP Docs</title>
<style>body{margin:0;padding:0;}redoc{display:block;}</style>
</head>
<body>
<redoc spec-url=\"openapi.json\"></redoc>
<script src=\"https://cdn.jsdelivr.net/npm/redoc@next/bundles/redoc.standalone.js\"></script>
</body>
</html>"#;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "ELF MCP Tools",
        version = "0.1.0",
        description = "Auto-generated reference for elf-mcp endpoints."
    ),
    paths(
        catalog_index_endpoint,
        list_bundles_endpoint,
        bundle_manifest_endpoint,
        open_resource_endpoint,
        list_devices_endpoint,
        simulate_run_endpoint,
        start_run_endpoint,
        tail_events_endpoint,
        derive_hrv_endpoint,
        signal_preview_endpoint,
        list_tools_endpoint
    )
)]
struct McpOpenApi;

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/catalog_index",
    tag = "tools",
    request_body = CatalogIndexParams,
    responses((status = 200, description = "Catalog summary", body = CatalogIndexResult))
)]
fn catalog_index_endpoint(_: CatalogIndexParams) -> CatalogIndexResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/list_bundles",
    tag = "tools",
    request_body = ListBundlesParams,
    responses((status = 200, description = "Bundles", body = ListBundlesResult))
)]
fn list_bundles_endpoint(_: ListBundlesParams) -> ListBundlesResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/bundle_manifest",
    tag = "tools",
    request_body = BundleManifestParams,
    responses((status = 200, description = "Run manifest", body = RunManifestDoc))
)]
fn bundle_manifest_endpoint(_: BundleManifestParams) -> RunManifestDoc {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/open_resource",
    tag = "tools",
    request_body = OpenResourceParams,
    responses((status = 200, description = "Resource payload", body = OpenResourceResult))
)]
fn open_resource_endpoint(_: OpenResourceParams) -> OpenResourceResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/list_devices",
    tag = "tools",
    request_body = ListDevicesParams,
    responses((status = 200, description = "Device catalog", body = DeviceCatalog))
)]
fn list_devices_endpoint(_: ListDevicesParams) -> DeviceCatalog {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/simulate_run",
    tag = "tools",
    request_body = SimulateRunParams,
    responses((status = 200, description = "Simulated bundle", body = SimulateRunResult))
)]
fn simulate_run_endpoint(_: SimulateRunParams) -> SimulateRunResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/start_run",
    tag = "tools",
    request_body = StartRunParams,
    responses((status = 200, description = "Start run response", body = StartRunResult))
)]
fn start_run_endpoint(_: StartRunParams) -> StartRunResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/tail_events",
    tag = "tools",
    request_body = TailEventsParams,
    responses((status = 200, description = "Event preview", body = TailEventsResult))
)]
fn tail_events_endpoint(_: TailEventsParams) -> TailEventsResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/derive_hrv",
    tag = "tools",
    request_body = DeriveHrvParams,
    responses((status = 200, description = "HRV metrics", body = DeriveHrvResult))
)]
fn derive_hrv_endpoint(_: DeriveHrvParams) -> DeriveHrvResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/signal_preview",
    tag = "tools",
    request_body = SignalPreviewParams,
    responses((status = 200, description = "Preview slice", body = SignalPreviewResult))
)]
fn signal_preview_endpoint(_: SignalPreviewParams) -> SignalPreviewResult {
    unreachable!()
}

#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/tools/list_tools",
    tag = "tools",
    request_body = ListToolsParams,
    responses((status = 200, description = "Schemas for all tools", body = ListToolsResult))
)]
fn list_tools_endpoint(_: ListToolsParams) -> ListToolsResult {
    unreachable!()
}
