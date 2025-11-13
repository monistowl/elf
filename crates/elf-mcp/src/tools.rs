use crate::{
    catalog::{BundleEntry, Catalog},
    resources::{Resource, ResourceResolver},
};
use anyhow::{anyhow, Result};
use log::info;
use serde_json::Value;

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
            "catalog_index",
            "list_bundles",
            "bundle_manifest",
            "open_resource",
            "list_devices",
            "simulate_run",
            "start_run",
            "tail_events",
            "derive_hrv",
            "signal_preview",
        ]
    }

    pub fn catalog_summary(&self) -> Value {
        self.catalog.to_json()
    }

    pub fn list_bundles(&self) -> Vec<BundleEntry> {
        self.catalog.bundles.clone()
    }

    pub fn manifest_for_run(&self, run_id: &str) -> Result<Resource> {
        let bundle = self
            .catalog
            .by_run_id(run_id)
            .ok_or_else(|| anyhow!("bundle {} not found", run_id))?;
        self.open_resource(&bundle.resource_uri("run.json"))
    }

    pub fn open_resource(&self, uri: &str) -> Result<Resource> {
        self.resolver.resolve(uri)
    }

    pub fn first_bundle(&self) -> Option<&BundleEntry> {
        self.catalog.first_bundle()
    }

    pub fn log_summary(&self) {
        info!("Registered tools: {:?}", Self::supported_tools());
        info!("Catalog entries: {}", self.catalog.bundles.len());
        if let Some(bundle) = self.first_bundle() {
            info!(
                "First bundle {} ({} events) at {}",
                bundle.run_id, bundle.total_events, bundle.bundle_path
            );
        }
        info!("Resource resolver available: {:p}", self.resolver);
    }
}
