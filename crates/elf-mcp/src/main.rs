mod catalog;
mod resources;
mod tools;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use env_logger::Env;
use log::info;

use crate::{catalog::Catalog, resources::ResourceResolver, tools::ToolRegistry};
use serde_json::json;
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(author, version, about = "elf-mcp MCP sidecar", long_about = None)]
struct Cli {
    /// Logging verbosity (e.g., debug, info, warn)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Transport to expose (stdio or websocket)
    #[arg(long, default_value = "stdio")]
    transport: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Probe catalog state (default)
    Probe,
    /// Print catalog summary JSON
    CatalogSummary,
    /// List discovered bundles
    ListBundles,
    /// Dump a bundle manifest
    BundleManifest {
        /// Run ID to query
        #[arg(long)]
        run: String,
    },
    /// Read an arbitrary resource by URI
    OpenResource {
        /// Resource URI (elf://...)
        #[arg(long)]
        uri: String,
    },
    /// Invoke any registered tool by name (JSON params)
    RunTool {
        /// Tool identifier (e.g., catalog_index)
        #[arg(long)]
        name: String,
        /// Optional JSON file for parameters
        #[arg(long)]
        params: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let mut args = Cli::parse();
    let command = args.command.take().unwrap_or_else(|| Command::Probe);

    env_logger::Builder::from_env(Env::default().default_filter_or(&args.log_level)).init();

    info!(
        "Starting elf-mcp with transport={} and log_level={}",
        args.transport, args.log_level
    );

    let catalog = Catalog::load()?;
    let resolver = ResourceResolver::new(&catalog);
    let registry = ToolRegistry::new(&catalog, &resolver);

    registry.log_summary();

    let summary = registry.catalog_summary();
    if let Some(count) = summary.get("count").and_then(|value| value.as_u64()) {
        info!("Catalog summary reports {} bundle(s)", count);
    }

    let bundles = registry.list_bundles();
    if !bundles.is_empty() {
        let ids: Vec<_> = bundles.iter().map(|bundle| bundle.run_id.clone()).collect();
        info!("Discovered bundle IDs: {:?}", ids);

        if let Some(bundle) = bundles.first() {
            if let Some(found) = catalog.by_run_id(&bundle.run_id) {
                info!(
                    "Verified bundle {} via catalog lookup (path={})",
                    found.run_id, found.bundle_path
                );
            }
        }
    }

    if let Ok(resource) = registry.open_resource("elf://catalog/index.json") {
        info!(
            "Catalog probe resource available: {} ({} bytes)",
            resource.uri,
            resource.data.len()
        );
    }

    if let Some(bundle) = registry.first_bundle() {
        if let Ok(resource) = registry.open_resource(&bundle.resource_uri("run.json")) {
            info!(
                "Manifest for bundle {} loaded ({} bytes)",
                bundle.run_id,
                resource.data.len()
            );
        }
    }

    match command {
        Command::Probe => info!("Probe command finished"),
        Command::CatalogSummary => {
            let response = registry.execute("catalog_index", None)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::ListBundles => {
            let response = registry.execute("list_bundles", None)?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::BundleManifest { run } => {
            let params = json!({ "run": run });
            let response = registry.execute("bundle_manifest", Some(params))?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::OpenResource { uri } => {
            let params = json!({ "uri": uri });
            let response = registry.execute("open_resource", Some(params))?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        Command::RunTool { name, params } => {
            let json_params = if let Some(path) = params {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                serde_json::from_str(&content)
                    .with_context(|| format!("parsing {}", path.display()))?
            } else {
                json!({})
            };
            let response = registry.execute(&name, Some(json_params))?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
    }

    Ok(())
}
