use crate::{
    catalog::{BundleEntry, Catalog},
    docs::DocRegistry,
    resources::{Resource, ResourceResolver},
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use elf_run::{
    read_design, read_trials, simulate_run as simulate_bundle, write_events_json, write_events_tsv,
    write_manifest,
};
use log::info;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tempfile::TempDir;

pub struct ToolRegistry<'a> {
    catalog: &'a Catalog,
    resolver: &'a ResourceResolver,
    docs: Arc<DocRegistry>,
    temp_dirs: RefCell<Vec<TempDir>>,
    temp_counter: AtomicUsize,
}

impl<'a> ToolRegistry<'a> {
    pub fn new(
        catalog: &'a Catalog,
        resolver: &'a ResourceResolver,
        docs: Arc<DocRegistry>,
    ) -> Self {
        Self {
            catalog,
            resolver,
            docs,
            temp_dirs: RefCell::new(Vec::new()),
            temp_counter: AtomicUsize::new(0),
        }
    }

    pub fn supported_tools() -> &'static [&'static str] {
        &[
            "catalog_index",
            "list_bundles",
            "bundle_manifest",
            "open_resource",
            "list_tools",
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

    pub fn execute(&self, tool: &str, params: Option<Value>) -> Result<Value> {
        let params = params.unwrap_or_else(|| json!({}));
        match tool {
            "catalog_index" => Ok(self.catalog_summary()),
            "list_bundles" => Ok(json!(self.list_bundles())),
            "list_tools" => Ok(json!(self.docs.list())),
            "bundle_manifest" => {
                let run = Self::require_param_str(&params, "run")?;
                let resource = self.manifest_for_run(&run)?;
                let manifest: Value =
                    serde_json::from_slice(&resource.data).context("parsing manifest JSON")?;
                Ok(manifest)
            }
            "open_resource" => {
                let uri = Self::require_param_str(&params, "uri")?;
                let resource = self.open_resource(&uri)?;
                Ok(json!({
                    "uri": resource.uri,
                    "bytes": resource.data.len(),
                    "base64": general_purpose::STANDARD.encode(&resource.data),
                }))
            }
            "simulate_run" => {
                let design_path = Self::require_param_str(&params, "design")?;
                let trials_path = Self::require_param_str(&params, "trials")?;
                let sub = Self::optional_param_str(&params, "sub").unwrap_or_else(|| "01".into());
                let ses = Self::optional_param_str(&params, "ses").unwrap_or_else(|| "01".into());
                let run_id =
                    Self::optional_param_str(&params, "run").unwrap_or_else(|| "01".into());

                let design = read_design(Path::new(&design_path))?;
                let trials = read_trials(Path::new(&trials_path))?;
                let bundle = simulate_bundle(&design, &trials, &sub, &ses, &run_id);

                let temp_dir = TempDir::new()?;
                let temp_path = temp_dir.path().to_path_buf();
                write_events_tsv(&temp_path.join("events.tsv"), &bundle.events)?;
                write_events_json(&temp_path.join("events.json"))?;
                write_manifest(&temp_path.join("run.json"), &bundle.manifest)?;

                let tmp_id = format!("tmp-{}", self.temp_counter.fetch_add(1, Ordering::SeqCst));
                let dir_display = temp_path.to_string_lossy().into_owned();
                self.temp_dirs.borrow_mut().push(temp_dir);
                self.resolver
                    .register_temp_bundle(&tmp_id, temp_path.clone());

                Ok(json!({
                    "bundle_id": bundle.manifest.run,
                    "sub": bundle.manifest.sub,
                    "ses": bundle.manifest.ses,
                    "task": bundle.manifest.task,
                    "design": bundle.manifest.design,
                    "resources": {
                        "events": format!("elf://tmp/{}/events.tsv", tmp_id),
                        "manifest": format!("elf://tmp/{}/run.json", tmp_id),
                        "metadata": format!("elf://tmp/{}/events.json", tmp_id),
                    },
                    "directory": dir_display,
                    "tmp_id": tmp_id,
                }))
            }
            _ => Err(anyhow!("unsupported tool {}", tool)),
        }
    }

    fn require_param_str(params: &Value, key: &str) -> Result<String> {
        params
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .ok_or_else(|| anyhow!("parameter '{}' is required", key))
    }

    fn optional_param_str(params: &Value, key: &str) -> Option<String> {
        params
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
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
