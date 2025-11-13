mod catalog;
mod docs;
mod resources;
mod tools;
mod transport;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use elf_keys;
use env_logger::Env;
use log::info;
use serde_json::json;
use std::{fs, path::PathBuf, sync::Arc};

use crate::{
    catalog::Catalog, docs::DocRegistry, resources::ResourceResolver, tools::ToolRegistry,
};

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
    /// Serve MCP requests over the configured transport.
    Serve,
    /// Manage TLS key/cert material for secure transports.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
}

#[derive(Subcommand, Clone)]
enum KeyCommand {
    /// List stored key/cert bundles
    List,
    /// Generate a new self-signed certificate
    Generate {
        /// Logical name for the key pair
        #[arg(long)]
        name: String,
        /// Validity period in days
        #[arg(long, default_value_t = 365)]
        days: u16,
    },
    /// Import an existing certificate + key
    Import {
        /// Logical name to store the bundle under
        #[arg(long)]
        name: String,
        /// Path to the certificate PEM
        #[arg(long)]
        cert: PathBuf,
        /// Path to the private key PEM
        #[arg(long)]
        key: PathBuf,
    },
    /// Export a key bundle to the given directory
    Export {
        /// Name of the key bundle to export
        #[arg(long)]
        name: String,
        /// Destination directory for the PEM files
        #[arg(long)]
        dest: PathBuf,
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

    let doc_registry = Arc::new(DocRegistry::default());
    let catalog = Catalog::load()?;
    let resolver = ResourceResolver::new(&catalog, doc_registry.clone());
    let registry = ToolRegistry::new(&catalog, &resolver, doc_registry.clone());

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
        Command::Serve => {
            info!("Starting MCP transport server ({})", args.transport);
            transport::run(&registry, &args.transport)?;
        }
        Command::Key { command } => match command {
            KeyCommand::List => {
                let keys = elf_keys::list_keys()?;
                println!("{}", serde_json::to_string_pretty(&keys)?);
            }
            KeyCommand::Generate { name, days } => {
                let entry = elf_keys::generate_key(&name, days)?;
                println!(
                    "Generated key {} (cert={}, key={})",
                    entry.name,
                    entry.cert_path.display(),
                    entry.key_path.display()
                );
            }
            KeyCommand::Import { name, cert, key } => {
                let entry = elf_keys::import_key(&name, &cert, &key)?;
                println!(
                    "Imported key {} (cert={}, key={})",
                    entry.name,
                    entry.cert_path.display(),
                    entry.key_path.display()
                );
            }
            KeyCommand::Export { name, dest } => {
                let (cert_path, key_path) = elf_keys::export_key(&name, &dest)?;
                println!(
                    "Exported {} -> cert={}, key={}",
                    name,
                    cert_path.display(),
                    key_path.display()
                );
            }
        },
    }

    Ok(())
}
