use crate::{
    catalog::{BundleEntry, Catalog},
    doc_types::{
        BundleDescriptor, BundleManifestParams, CatalogIndexParams, CatalogIndexResult,
        DeriveHrvParams, DeriveHrvResult, DeviceCatalog, DeviceDescriptor, EventRecord,
        ListBundlesParams, ListBundlesResult, ListDevicesParams, ListToolsParams, ListToolsResult,
        OpenResourceParams, OpenResourceResult, RunManifestDoc, SignalPreviewParams,
        SignalPreviewResult, SimulateRunParams, SimulateRunResult, SimulatedBundleResources,
        StartMode, StartRunParams, StartRunResult, TailEventsParams, TailEventsResult,
    },
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

    pub fn catalog_summary(&self) -> CatalogIndexResult {
        let bundles: Vec<BundleDescriptor> = self
            .catalog
            .bundles
            .iter()
            .map(BundleDescriptor::from)
            .collect();
        CatalogIndexResult {
            count: bundles.len(),
            bundles,
        }
    }

    pub fn list_bundles(&self) -> Vec<BundleEntry> {
        self.catalog.bundles.clone()
    }

    pub fn manifest_for_run(&self, run_id: &str) -> Result<RunManifestDoc> {
        let bundle = self
            .catalog
            .by_run_id(run_id)
            .ok_or_else(|| anyhow!("bundle {} not found", run_id))?;
        let resource = self.open_resource(&bundle.resource_uri("run.json"))?;
        let manifest = serde_json::from_slice::<RunManifestDoc>(&resource.data)
            .context("parsing manifest JSON")?;
        Ok(manifest)
    }

    pub fn open_resource(&self, uri: &str) -> Result<Resource> {
        self.resolver.resolve(uri)
    }

    pub fn execute(&self, tool: &str, params: Option<Value>) -> Result<Value> {
        let params = params.unwrap_or_else(|| json!({}));
        match tool {
            "catalog_index" => {
                serde_json::from_value::<CatalogIndexParams>(params)?;
                let summary = self.catalog_summary();
                Ok(serde_json::to_value(summary)?)
            }
            "list_bundles" => {
                serde_json::from_value::<ListBundlesParams>(params)?;
                let bundles: ListBundlesResult = self
                    .catalog
                    .bundles
                    .iter()
                    .map(BundleDescriptor::from)
                    .collect();
                Ok(serde_json::to_value(bundles)?)
            }
            "list_tools" => {
                serde_json::from_value::<ListToolsParams>(params)?;
                let tools: ListToolsResult = self.docs.list();
                Ok(serde_json::to_value(tools)?)
            }
            "list_devices" => {
                serde_json::from_value::<ListDevicesParams>(params)?;
                let catalog = Self::device_catalog();
                Ok(serde_json::to_value(catalog)?)
            }
            "bundle_manifest" => {
                let input: BundleManifestParams = serde_json::from_value(params)?;
                let manifest = self.manifest_for_run(&input.run)?;
                Ok(serde_json::to_value(manifest)?)
            }
            "open_resource" => {
                let input: OpenResourceParams = serde_json::from_value(params)?;
                let resource = self.open_resource(&input.uri)?;
                let payload = OpenResourceResult {
                    uri: resource.uri,
                    bytes: resource.data.len(),
                    base64: general_purpose::STANDARD.encode(&resource.data),
                };
                Ok(serde_json::to_value(payload)?)
            }
            "simulate_run" => {
                let input: SimulateRunParams = serde_json::from_value(params)?;
                let (sub, ses, run_id) = Self::resolve_run_parts(
                    input.sub.as_deref(),
                    input.ses.as_deref(),
                    input.run.as_deref(),
                );
                let result =
                    self.materialize_bundle(&input.design, &input.trials, &sub, &ses, &run_id)?;
                Ok(serde_json::to_value(result)?)
            }
            "start_run" => {
                let input: StartRunParams = serde_json::from_value(params)?;
                if !input.dry_run && !input.confirm {
                    return Err(anyhow!(
                        "start_run requires confirm=true when dry_run=false"
                    ));
                }
                let base = &input.base;
                let (sub, ses, run_id) = Self::resolve_run_parts(
                    base.sub.as_deref(),
                    base.ses.as_deref(),
                    base.run.as_deref(),
                );
                let bundle =
                    self.materialize_bundle(&base.design, &base.trials, &sub, &ses, &run_id)?;
                let mode = if input.dry_run {
                    StartMode::DryRun
                } else {
                    StartMode::Live
                };
                let result = StartRunResult {
                    bundle,
                    dry_run: input.dry_run,
                    mode,
                    devices: input.devices.clone(),
                };
                Ok(serde_json::to_value(result)?)
            }
            "tail_events" => {
                let input: TailEventsParams = serde_json::from_value(params)?;
                let since = input.since.unwrap_or(0.0);
                let limit = input.limit.unwrap_or(50);
                let (events, resource_uri) =
                    self.events_for_source(input.run.as_deref(), input.tmp_id.as_deref())?;
                let preview: Vec<EventRecord> = events
                    .into_iter()
                    .filter(|event| event.onset >= since)
                    .take(limit)
                    .collect();
                let response = TailEventsResult {
                    source: resource_uri,
                    count: preview.len(),
                    events: preview,
                };
                Ok(serde_json::to_value(response)?)
            }
            "derive_hrv" => {
                let input: DeriveHrvParams = serde_json::from_value(params)?;
                let stream = input
                    .stream
                    .or_else(|| input.run.clone())
                    .unwrap_or_else(|| "unknown".into());
                let (events, resource_uri) =
                    self.events_for_source(input.run.as_deref(), input.tmp_id.as_deref())?;
                let rr_series = Self::rr_series_from_events(&events)?;
                let time_stats = hrv_time(&rr_series).into();
                let psd_stats = hrv_psd(&rr_series, 4.0).into();
                let nl_stats = hrv_nonlinear(&rr_series).into();
                let response = DeriveHrvResult {
                    stream,
                    source: resource_uri,
                    rr_series: rr_series.rr,
                    hrv_time: time_stats,
                    hrv_psd: psd_stats,
                    hrv_nonlinear: nl_stats,
                };
                Ok(serde_json::to_value(response)?)
            }
            "signal_preview" => {
                let input: SignalPreviewParams = serde_json::from_value(params)?;
                let stream = input
                    .stream
                    .or_else(|| input.run.clone())
                    .unwrap_or_else(|| "events".into());
                let tmin = input.tmin.unwrap_or(f64::MIN);
                let tmax = input.tmax.unwrap_or(f64::MAX);
                let decimate = input.decimate.unwrap_or(1).max(1);
                let (events, resource_uri) =
                    self.events_for_source(input.run.as_deref(), input.tmp_id.as_deref())?;
                let preview: Vec<EventRecord> = events
                    .into_iter()
                    .filter(|event| event.onset >= tmin && event.onset <= tmax)
                    .enumerate()
                    .filter(|(idx, _)| idx % decimate == 0)
                    .map(|(_, event)| event)
                    .collect();
                let response = SignalPreviewResult {
                    stream,
                    source: resource_uri,
                    tmin,
                    tmax,
                    decimate,
                    events: preview,
                    annotations: input.annotations.clone(),
                };
                Ok(serde_json::to_value(response)?)
            }
            _ => Err(anyhow!("unsupported tool {}", tool)),
        }
    }

    fn device_catalog() -> DeviceCatalog {
        DeviceCatalog {
            devices: vec![
                DeviceDescriptor {
                    id: "lsl:EEG-01".into(),
                    name: "EEG Cap".into(),
                    kind: "lsl".into(),
                    channels: 64,
                    sampling_rate: Some(500.0),
                    description: Some("Standard EEG cap streamed over LSL".into()),
                },
                DeviceDescriptor {
                    id: "lsl:ECG-01".into(),
                    name: "ECG Lead".into(),
                    kind: "lsl".into(),
                    channels: 1,
                    sampling_rate: Some(1000.0),
                    description: Some("Single-lead ECG via Bitalino".into()),
                },
                DeviceDescriptor {
                    id: "trigger:audio".into(),
                    name: "Audio Trigger".into(),
                    kind: "trigger".into(),
                    channels: 1,
                    sampling_rate: None,
                    description: Some("OS-level audio trigger output".into()),
                },
            ],
        }
    }

    fn resolve_run_parts(
        sub: Option<&str>,
        ses: Option<&str>,
        run: Option<&str>,
    ) -> (String, String, String) {
        let sub = sub.unwrap_or("01").to_string();
        let ses = ses.unwrap_or("01").to_string();
        let run_id = run.unwrap_or("01").to_string();
        (sub, ses, run_id)
    }

    fn materialize_bundle(
        &self,
        design_path: &str,
        trials_path: &str,
        sub: &str,
        ses: &str,
        run_id: &str,
    ) -> Result<SimulateRunResult> {
        let design = read_design(Path::new(design_path))?;
        let trials = read_trials(Path::new(trials_path))?;
        let elf_run::RunBundle { events, manifest } =
            simulate_bundle(&design, &trials, sub, ses, run_id);

        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path().to_path_buf();
        write_events_tsv(&temp_path.join("events.tsv"), &events)?;
        write_events_json(&temp_path.join("events.json"))?;
        write_manifest(&temp_path.join("run.json"), &manifest)?;

        let tmp_id = format!("tmp-{}", self.temp_counter.fetch_add(1, Ordering::SeqCst));
        let dir_display = temp_path.to_string_lossy().into_owned();
        self.temp_dirs.borrow_mut().push(temp_dir);
        self.resolver
            .register_temp_bundle(&tmp_id, temp_path.clone());

        let elf_run::RunManifest {
            sub,
            ses,
            run,
            task,
            design,
            ..
        } = manifest;
        let resources = SimulatedBundleResources {
            events: format!("elf://tmp/{}/events.tsv", tmp_id),
            manifest: format!("elf://tmp/{}/run.json", tmp_id),
            metadata: format!("elf://tmp/{}/events.json", tmp_id),
        };

        Ok(SimulateRunResult {
            bundle_id: run,
            sub,
            ses,
            task,
            design,
            resources,
            directory: dir_display,
            tmp_id,
        })
    }

    fn events_for_source(
        &self,
        run: Option<&str>,
        tmp_id: Option<&str>,
    ) -> Result<(Vec<EventRecord>, String)> {
        if let Some(tmp) = tmp_id {
            let base_path = self
                .resolver
                .temp_base_path(tmp)
                .ok_or_else(|| anyhow!("tmp bundle {} not registered", tmp))?;
            let events = Self::read_events_file(&base_path.join("events.tsv"))?;
            return Ok((events, format!("elf://tmp/{}/events.tsv", tmp)));
        }

        let run_id = run
            .map(|value| value.to_string())
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
