mod catalog;
mod resources;
mod tools;

use anyhow::Result;
use clap::Parser;
use env_logger::Env;
use log::info;

use crate::{catalog::Catalog, resources::ResourceResolver, tools::ToolRegistry};

#[derive(Parser)]
#[command(author, version, about = "elf-mcp MCP sidecar", long_about = None)]
struct Cli {
    /// Logging verbosity (e.g., debug, info, warn)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Transport to expose (stdio or websocket)
    #[arg(long, default_value = "stdio")]
    transport: String,
}

fn main() -> Result<()> {
    let args = Cli::parse();
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

    Ok(())
}
