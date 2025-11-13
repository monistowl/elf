use crate::catalog::BundleEntry;
use elf_lib::metrics::hrv::{HRVNonlinear, HRVPsd, HRVTime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use utoipa::ToSchema;

/// Shared empty object schema for parameter-less tools.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyObject {}

/// Public bundle metadata exposed via MCP docs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct BundleDescriptor {
    pub run_id: String,
    pub subject: String,
    pub session: String,
    pub task: String,
    pub design: String,
    pub total_trials: usize,
    pub total_events: usize,
    pub seed: Option<u64>,
    pub randomization_policy: Option<String>,
    pub isi_ms: f64,
    pub isi_jitter_ms: Option<f64>,
    pub started: f64,
    pub bundle_path: String,
}

impl From<&BundleEntry> for BundleDescriptor {
    fn from(entry: &BundleEntry) -> Self {
        Self {
            run_id: entry.run_id.clone(),
            subject: entry.subject.clone(),
            session: entry.session.clone(),
            task: entry.task.clone(),
            design: entry.design.clone(),
            total_trials: entry.total_trials,
            total_events: entry.total_events,
            seed: entry.seed,
            randomization_policy: entry.randomization_policy.clone(),
            isi_ms: entry.isi_ms,
            isi_jitter_ms: entry.isi_jitter_ms,
            started: entry.started,
            bundle_path: entry.bundle_path.clone(),
        }
    }
}

impl From<BundleEntry> for BundleDescriptor {
    fn from(entry: BundleEntry) -> Self {
        BundleDescriptor::from(&entry)
    }
}

pub type CatalogIndexParams = EmptyObject;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct CatalogIndexResult {
    pub bundles: Vec<BundleDescriptor>,
    pub count: usize,
}

pub type ListBundlesParams = EmptyObject;
pub type ListBundlesResult = Vec<BundleDescriptor>;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleManifestParams {
    pub run: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct RunManifestDoc {
    pub sub: String,
    pub ses: String,
    pub run: String,
    pub task: String,
    pub design: String,
    pub total_trials: usize,
    pub total_events: usize,
    pub seed: Option<u64>,
    pub randomization_policy: Option<String>,
    pub isi_ms: f64,
    pub isi_jitter_ms: Option<f64>,
    pub start_time_unix: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenResourceParams {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct OpenResourceResult {
    pub uri: String,
    pub bytes: usize,
    pub base64: String,
}

pub type ListDevicesParams = EmptyObject;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct DeviceDescriptor {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub channels: usize,
    pub sampling_rate: Option<f64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct DeviceCatalog {
    pub devices: Vec<DeviceDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SimulateRunParams {
    pub design: String,
    pub trials: String,
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub ses: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartRunParams {
    #[serde(flatten)]
    pub base: SimulateRunParams,
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub devices: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct SimulatedBundleResources {
    pub events: String,
    pub manifest: String,
    pub metadata: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct SimulateRunResult {
    pub bundle_id: String,
    pub sub: String,
    pub ses: String,
    pub task: String,
    pub design: String,
    pub resources: SimulatedBundleResources,
    pub directory: String,
    pub tmp_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct StartRunResult {
    #[serde(flatten)]
    pub bundle: SimulateRunResult,
    pub dry_run: bool,
    pub mode: StartMode,
    #[serde(default)]
    pub devices: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub enum StartMode {
    #[serde(rename = "dry_run")]
    DryRun,
    #[serde(rename = "live")]
    Live,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TailEventsParams {
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub tmp_id: Option<String>,
    #[serde(default)]
    pub since: Option<f64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct EventRecord {
    pub onset: f64,
    pub duration: f64,
    pub trial: usize,
    pub block: usize,
    pub event_type: String,
    pub stim_id: String,
    pub condition: String,
    pub resp_key: Option<String>,
    pub resp_rt: Option<f64>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct TailEventsResult {
    pub source: String,
    pub count: usize,
    pub events: Vec<EventRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeriveHrvParams {
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub tmp_id: Option<String>,
    #[serde(default)]
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct HrvTimeStats {
    pub n: usize,
    pub avnn: f64,
    pub sdnn: f64,
    pub rmssd: f64,
    pub pnn50: f64,
}

impl From<HRVTime> for HrvTimeStats {
    fn from(value: HRVTime) -> Self {
        Self {
            n: value.n,
            avnn: value.avnn,
            sdnn: value.sdnn,
            rmssd: value.rmssd,
            pnn50: value.pnn50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct HrvPsdStats {
    pub lf: f64,
    pub hf: f64,
    pub vlf: f64,
    pub lf_hf: f64,
    pub total_power: f64,
    pub points: Vec<[f64; 2]>,
}

impl From<HRVPsd> for HrvPsdStats {
    fn from(value: HRVPsd) -> Self {
        Self {
            lf: value.lf,
            hf: value.hf,
            vlf: value.vlf,
            lf_hf: value.lf_hf,
            total_power: value.total_power,
            points: value.points,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct HrvNonlinearStats {
    pub sd1: f64,
    pub sd2: f64,
    pub samp_entropy: f64,
    pub dfa_alpha1: f64,
}

impl From<HRVNonlinear> for HrvNonlinearStats {
    fn from(value: HRVNonlinear) -> Self {
        Self {
            sd1: value.sd1,
            sd2: value.sd2,
            samp_entropy: value.samp_entropy,
            dfa_alpha1: value.dfa_alpha1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct DeriveHrvResult {
    pub stream: String,
    pub source: String,
    pub rr_series: Vec<f64>,
    pub hrv_time: HrvTimeStats,
    pub hrv_psd: HrvPsdStats,
    pub hrv_nonlinear: HrvNonlinearStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SignalPreviewParams {
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub tmp_id: Option<String>,
    #[serde(default)]
    pub stream: Option<String>,
    #[serde(default)]
    pub tmin: Option<f64>,
    #[serde(default)]
    pub tmax: Option<f64>,
    #[serde(default)]
    pub decimate: Option<usize>,
    #[serde(default)]
    pub annotations: Option<Vec<AnnotationEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct AnnotationEvent {
    pub onset: f64,
    pub label: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct SignalPreviewResult {
    pub stream: String,
    pub source: String,
    pub tmin: f64,
    pub tmax: f64,
    pub decimate: usize,
    pub events: Vec<EventRecord>,
    #[serde(default)]
    pub annotations: Option<Vec<AnnotationEvent>>,
}

pub type ListToolsParams = EmptyObject;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct ToolDescription {
    pub name: String,
    pub summary: String,
    pub params_schema: JsonValue,
    pub result_schema: JsonValue,
    pub help: String,
}

pub type ListToolsResult = Vec<ToolDescription>;
