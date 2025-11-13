use crate::{catalog::Catalog, resources::ResourceResolver};
use log::info;

pub struct ToolRegistry<'a> {
    catalog: &'a Catalog,
    resolver: &'a ResourceResolver,
}

impl<'a> ToolRegistry<'a> {
    pub fn new(catalog: &'a Catalog, resolver: &'a ResourceResolver) -> Self {
        Self { catalog, resolver }
    }

    pub fn supported_tools() -> &'static [&'static str] {
        &[
            "list_devices",
            "validate_design",
            "simulate_run",
            "start_run",
            "tail_events",
            "list_bundles",
            "open_resource",
            "derive_hrv",
            "bundle_manifest",
            "signal_preview",
        ]
    }

    pub fn log_summary(&self) {
        info!("Registered tools: {:?}", Self::supported_tools());
        info!("Catalog entries: {}", self.catalog.bundles.len());
        info!("Resource resolver available: {:p}", self.resolver);
    }
}
