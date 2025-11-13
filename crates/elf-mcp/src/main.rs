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
        args.transport,
        args.log_level
    );

    let catalog = Catalog::load()?;
    let resolver = ResourceResolver::new();

    if let Ok(resource) = resolver.resolve("elf://catalog/index.json") {
        info!(
            "Catalog probe resource available: {} ({} bytes)",
            resource.uri,
            resource.data.len()
        );
    }

    let registry = ToolRegistry::new(&catalog, &resolver);

    registry.log_summary();

    Ok(())
}
