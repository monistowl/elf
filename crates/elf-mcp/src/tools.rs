use crate::{
    catalog::{BundleEntry, Catalog},
    docs::DocRegistry,
    resources::{Resource, ResourceResolver},
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use csv::{ReaderBuilder, Trim};
use elf_lib::{
    metrics::hrv::{hrv_nonlinear, hrv_psd, hrv_time},
    signal::RRSeries,
};
use elf_run::{
    read_design, read_trials, simulate_run as simulate_bundle, write_events_json, write_events_tsv,
    write_manifest,
};
use log::info;
use serde::Deserialize;
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
            "list_devices" => Ok(Self::device_catalog()),
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
                self.materialize_bundle(&design_path, &trials_path, &sub, &ses, &run_id)
            }
            "start_run" => {
                let design_path = Self::require_param_str(&params, "design")?;
                let trials_path = Self::require_param_str(&params, "trials")?;
                let sub = Self::optional_param_str(&params, "sub").unwrap_or_else(|| "01".into());
                let ses = Self::optional_param_str(&params, "ses").unwrap_or_else(|| "01".into());
                let run_id =
                    Self::optional_param_str(&params, "run").unwrap_or_else(|| "01".into());
                let dry_run = params
                    .get("dry_run")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(true);
                let confirm = params
                    .get("confirm")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);

                if !dry_run && !confirm {
                    return Err(anyhow!(
                        "start_run requires confirm=true when dry_run=false"
                    ));
                }

                let mut response =
                    self.materialize_bundle(&design_path, &trials_path, &sub, &ses, &run_id)?;
                if let Some(map) = response.as_object_mut() {
                    map.insert("dry_run".to_string(), json!(dry_run));
                    map.insert(
                        "mode".to_string(),
                        json!(if dry_run { "dry_run" } else { "live" }),
                    );
                    if let Some(devices) = params.get("devices") {
                        map.insert("devices".to_string(), devices.clone());
                    }
                }
                Ok(response)
            }
            "tail_events" => {
                let since = params
                    .get("since")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(0.0);
                let limit = params
                    .get("limit")
                    .and_then(|value| value.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(50);
                let (events, resource_uri) = self.events_for_params(&params)?;
                let preview: Vec<Value> = events
                    .into_iter()
                    .filter(|event| event.onset >= since)
                    .take(limit)
                    .map(|event| event.to_json())
                    .collect();
                Ok(json!({
                    "source": resource_uri,
                    "count": preview.len(),
                    "events": preview,
                }))
            }
            "derive_hrv" => {
                let stream = Self::optional_param_str(&params, "stream").unwrap_or_else(|| {
                    Self::optional_param_str(&params, "run").unwrap_or_else(|| "unknown".into())
                });
                let (events, resource_uri) = self.events_for_params(&params)?;
                let rr_series = Self::rr_series_from_events(&events)?;
                let time_stats = hrv_time(&rr_series);
                let psd_stats = hrv_psd(&rr_series, 4.0);
                let nl_stats = hrv_nonlinear(&rr_series);
                Ok(json!({
                    "stream": stream,
                    "source": resource_uri,
                    "rr_series": rr_series.rr,
                    "hrv_time": time_stats,
                    "hrv_psd": psd_stats,
                    "hrv_nonlinear": nl_stats,
                }))
            }
            "signal_preview" => {
                let stream = Self::optional_param_str(&params, "stream").unwrap_or_else(|| {
                    Self::optional_param_str(&params, "run").unwrap_or_else(|| "events".into())
                });
                let tmin = params
                    .get("tmin")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(f64::MIN);
                let tmax = params
                    .get("tmax")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(f64::MAX);
                let decimate = params
                    .get("decimate")
                    .and_then(|value| value.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(1)
                    .max(1);
                let (events, resource_uri) = self.events_for_params(&params)?;
                let preview: Vec<Value> = events
                    .into_iter()
                    .filter(|event| event.onset >= tmin && event.onset <= tmax)
                    .enumerate()
                    .filter(|(idx, _)| idx % decimate == 0)
                    .map(|(_, event)| event.to_json())
                    .collect();
                let annotations = params.get("annotations").cloned();
                Ok(json!({
                    "stream": stream,
                    "source": resource_uri,
                    "tmin": tmin,
                    "tmax": tmax,
                    "decimate": decimate,
                    "events": preview,
                    "annotations": annotations,
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

    fn device_catalog() -> Value {
        json!({
            "devices": [
                {
                    "id": "lsl:EEG-01",
                    "name": "EEG Cap",
                    "type": "lsl",
                    "channels": 64,
                    "sampling_rate": 500.0,
                    "description": "Standard EEG cap streamed over LSL"
                },
                {
                    "id": "lsl:ECG-01",
                    "name": "ECG Lead",
                    "type": "lsl",
                    "channels": 1,
                    "sampling_rate": 1000.0,
                    "description": "Single-lead ECG via Bitalino"
                },
                {
                    "id": "trigger:audio",
                    "name": "Audio Trigger",
                    "type": "trigger",
                    "channels": 1,
                    "description": "OS-level audio trigger output"
                }
            ]
        })
    }

    fn materialize_bundle(
        &self,
        design_path: &str,
        trials_path: &str,
        sub: &str,
        ses: &str,
        run_id: &str,
    ) -> Result<Value> {
        let design = read_design(Path::new(design_path))?;
        let trials = read_trials(Path::new(trials_path))?;
        let bundle = simulate_bundle(&design, &trials, sub, ses, run_id);

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

    fn events_for_params(&self, params: &Value) -> Result<(Vec<EventRecord>, String)> {
        if let Some(tmp_id) = Self::optional_param_str(params, "tmp_id") {
            let base_path = self
                .resolver
                .temp_base_path(&tmp_id)
                .ok_or_else(|| anyhow!("tmp bundle {} not registered", tmp_id))?;
            let events = Self::read_events_file(&base_path.join("events.tsv"))?;
            return Ok((events, format!("elf://tmp/{}/events.tsv", tmp_id)));
        }

        let run_id = Self::optional_param_str(params, "run")
            .or_else(|| self.first_bundle().map(|bundle| bundle.run_id.clone()))
            .ok_or_else(|| anyhow!("run or tmp_id parameter is required"))?;
        let bundle = self
            .catalog
            .by_run_id(&run_id)
            .ok_or_else(|| anyhow!("bundle {} not found", run_id))?;
        let events = Self::read_events_file(&bundle.path.join("events.tsv"))?;
        Ok((events, bundle.resource_uri("events.tsv")))
    }

    fn read_events_file(path: &Path) -> Result<Vec<EventRecord>> {
        let mut reader = ReaderBuilder::new()
            .delimiter(b'\t')
            .trim(Trim::All)
            .from_path(path)
            .with_context(|| format!("reading events {}", path.display()))?;
        let mut events = Vec::new();
        for record in reader.deserialize::<EventRecord>() {
            events.push(record?);
        }
        Ok(events)
    }

    fn rr_series_from_events(events: &[EventRecord]) -> Result<RRSeries> {
        let mut stim_onsets: Vec<f64> = events
            .iter()
            .filter(|event| event.event_type == "stim")
            .map(|event| event.onset)
            .collect();
        stim_onsets.dedup();
        if stim_onsets.len() < 2 {
            anyhow::bail!("not enough stim events for HRV");
        }
        let rr: Vec<f64> = stim_onsets
            .windows(2)
            .map(|window| (window[1] - window[0]).max(0.0))
            .collect();
        Ok(RRSeries { rr })
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

#[derive(Debug, Deserialize)]
struct EventRecord {
    onset: f64,
    duration: f64,
    trial: usize,
    block: usize,
    event_type: String,
    stim_id: String,
    condition: String,
    resp_key: Option<String>,
    resp_rt: Option<f64>,
    value: Option<String>,
}

impl EventRecord {
    fn to_json(&self) -> Value {
        json!({
            "onset": self.onset,
            "duration": self.duration,
            "trial": self.trial,
            "block": self.block,
            "event_type": self.event_type,
            "stim_id": self.stim_id,
            "condition": self.condition,
            "resp_key": self.resp_key,
            "resp_rt": self.resp_rt,
            "value": self.value,
        })
    }
}
