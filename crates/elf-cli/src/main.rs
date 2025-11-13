use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use elf_lib::{
    detectors::ecg::{
        detect_r_peaks, run_beat_hrv_pipeline, BeatHrvPipelineResult, EcgPipelineConfig,
    },
    io::{
        bitalino as bitalino_io, eeg as eeg_io, eye as eye_io, openbci as openbci_io,
        text as text_io, wfdb as wfdb_io,
    },
    metrics::{
        hrv::{hrv_nonlinear, hrv_psd, hrv_time, HRVPsd, HRVTime},
        sqi::evaluate_sqi,
    },
    plot::{figure_from_rr, Figure, Series},
    signal::{Events, RRSeries, TimeSeries},
};
use elf_run::{
    read_design, read_trials, simulate_run, write_events_json, write_events_tsv, write_manifest,
};
use plotters::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};
fn ensure_run_bundle(repo_root: &Path) -> Result<()> {
    let bundle_dir = repo_root.join("test_data/run_bundle");
    if bundle_dir.join("events.idx").exists() {
        return Ok(());
    }
    let script = repo_root.join("scripts/generate_run_bundle.sh");
    let status = Command::new(script)
        .current_dir(repo_root)
        .status()
        .context("running run bundle generator")?;
    if !status.success() {
        anyhow::bail!("run bundle generator failed")
    }
    Ok(())
}

#[derive(Parser)]
#[command(
    name = "elf",
    version,
    about = "ELF: Extensible Lab Framework CLI tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum EyeFormat {
    #[value(name = "pupil-labs")]
    PupilLabs,
    #[value(name = "tobii")]
    Tobii,
}

impl EyeFormat {
    fn columns(
        &self,
    ) -> (
        &'static str,
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        u8,
    ) {
        match self {
            EyeFormat::PupilLabs => (
                "timestamp",
                "diameter",
                Some("confidence"),
                Some("eye"),
                b',',
            ),
            EyeFormat::Tobii => (
                "system_time_stamp",
                "pupil_diameter_2d",
                Some("confidence"),
                Some("eye"),
                b'\t',
            ),
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Detect R-peaks from newline-delimited samples read from stdin or --input file
    EcgFindRpeaks {
        #[arg(long, default_value_t = 250.0)]
        fs: f64,
        #[arg(long, default_value_t = 0.12)]
        min_rr_s: f64,
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        wfdb_header: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        wfdb_lead: usize,
        #[arg(long)]
        eeg_edf: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        eeg_channel: usize,
    },
    /// Compute time-domain HRV from newline-delimited RR intervals (seconds)
    HrvTime {
        #[arg(long)]
        input: Option<PathBuf>,
    },
    /// Run beat detection → RR series → HRV in one shot
    BeatHrvPipeline {
        #[arg(long, default_value_t = 250.0)]
        fs: f64,
        #[arg(long, default_value_t = 5.0)]
        lowcut_hz: f64,
        #[arg(long, default_value_t = 15.0)]
        highcut_hz: f64,
        #[arg(long, default_value_t = 0.150)]
        integration_window_s: f64,
        #[arg(long, default_value_t = 0.12)]
        min_rr_s: f64,
        #[arg(long, default_value_t = 0.6)]
        threshold_scale: f64,
        #[arg(long, default_value_t = 0.150)]
        search_back_s: f64,
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        wfdb_header: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        wfdb_lead: usize,
        #[arg(long)]
        annotations: Option<PathBuf>,
        #[arg(long)]
        eeg_edf: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        eeg_channel: usize,
        #[arg(long)]
        bids_events: Option<PathBuf>,
    },
    /// Normalize pupil exports (Pupil Labs/Tobii) and filter by confidence
    PupilNormalize {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "pupil-labs")]
        format: EyeFormat,
        #[arg(long, default_value_t = 0.5)]
        min_confidence: f32,
    },
    /// Frequency-domain HRV (Welch PSD)
    HrvPsd {
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long, default_value_t = 4.0)]
        interp_fs: f64,
    },
    /// Nonlinear HRV metrics (Poincaré, SampEn)
    HrvNonlinear {
        #[arg(long)]
        input: Option<PathBuf>,
    },
    /// Render RR series to a PNG via plotters
    HrvPlot {
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Simulate a run from design + trial specs and emit events/manifest bundle
    RunSimulate {
        #[arg(long)]
        design: PathBuf,
        #[arg(long)]
        trials: PathBuf,
        #[arg(long, default_value = "01")]
        sub: String,
        #[arg(long, default_value = "01")]
        ses: String,
        #[arg(long, default_value = "01")]
        run: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Load a BITalino / OpenSignals CSV and run the ECG HRV pipeline
    Bitalino {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "analog0")]
        signal: String,
        #[arg(long)]
        fs: Option<f64>,
    },
    /// Load OpenBCI CSV and run the ECG HRV pipeline
    OpenBci {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "Ch1")]
        channel: String,
        #[arg(long)]
        fs: Option<f64>,
    },
    /// Validate a dataset spec JSON against the computed HRV summaries
    DatasetValidate {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Compute signal quality indices (kurtosis, SNR, RR coefficient of variation)
    Sqi {
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long, default_value_t = 250.0)]
        fs: f64,
    },
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::EcgFindRpeaks {
            fs,
            min_rr_s,
            input,
            wfdb_header,
            wfdb_lead,
            eeg_edf,
            eeg_channel,
        } => cmd_ecg_find_rpeaks(
            fs,
            min_rr_s,
            input.as_deref(),
            wfdb_header.as_deref(),
            wfdb_lead,
            eeg_edf.as_deref(),
            eeg_channel,
        )?,
        Commands::HrvTime { input } => cmd_hrv_time(input.as_deref())?,
        Commands::BeatHrvPipeline {
            fs,
            lowcut_hz,
            highcut_hz,
            integration_window_s,
            min_rr_s,
            threshold_scale,
            search_back_s,
            input,
            wfdb_header,
            wfdb_lead,
            annotations,
            eeg_edf,
            eeg_channel,
            bids_events,
        } => cmd_beat_hrv_pipeline(
            fs,
            lowcut_hz,
            highcut_hz,
            integration_window_s,
            min_rr_s,
            threshold_scale,
            search_back_s,
            input.as_deref(),
            wfdb_header.as_deref(),
            wfdb_lead,
            annotations.as_deref(),
            eeg_edf.as_deref(),
            eeg_channel,
            bids_events.as_deref(),
        )?,
        Commands::PupilNormalize {
            input,
            format,
            min_confidence,
        } => cmd_pupil_normalize(&input, format, min_confidence)?,
        Commands::HrvPsd { input, interp_fs } => cmd_hrv_psd(input.as_deref(), interp_fs)?,
        Commands::HrvNonlinear { input } => cmd_hrv_nonlinear(input.as_deref())?,
        Commands::HrvPlot { input, out } => cmd_hrv_plot(input.as_deref(), &out)?,
        Commands::Bitalino { input, signal, fs } => {
            cmd_bitalino_hrv(&input, &signal, fs.unwrap_or(0.0))?
        }
        Commands::OpenBci { input, channel, fs } => {
            cmd_openbci_hrv(&input, &channel, fs.unwrap_or(0.0))?
        }
        Commands::DatasetValidate { spec, json } => cmd_dataset_validate(&spec, json.as_deref())?,
        Commands::Sqi { input, fs } => cmd_sqi(input.as_deref(), fs)?,
        Commands::RunSimulate {
            design,
            trials,
            sub,
            ses,
            run,
            out,
        } => cmd_run_simulate(&design, &trials, &sub, &ses, &run, &out)?,
    }
    Ok(())
}

fn read_samples(input: Option<&Path>) -> Result<Vec<f64>> {
    match input {
        Some(path) => text_io::read_f64_series(path),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            text_io::parse_f64_series(&buf)
        }
    }
}

fn rr_series_from_input(input: Option<&Path>) -> Result<RRSeries> {
    let rr = read_samples(input)?;
    Ok(RRSeries { rr })
}

fn cmd_hrv_psd(input: Option<&Path>, interp_fs: f64) -> Result<()> {
    let rr = rr_series_from_input(input)?;
    let psd = hrv_psd(&rr, interp_fs);
    println!("{}", serde_json::to_string(&psd)?);
    Ok(())
}

fn cmd_hrv_nonlinear(input: Option<&Path>) -> Result<()> {
    let rr = rr_series_from_input(input)?;
    let nonlinear = hrv_nonlinear(&rr);
    println!("{}", serde_json::to_string(&nonlinear)?);
    Ok(())
}

fn cmd_hrv_plot(input: Option<&Path>, out: &Path) -> Result<()> {
    let rr = rr_series_from_input(input)?;
    let fig = figure_from_rr(&rr);
    draw_plotters_figure(out, &fig)?;
    Ok(())
}

fn cmd_bitalino_hrv(path: &Path, signal: &str, fs_override: f64) -> Result<()> {
    let mut ts = bitalino_io::read_bitalino_csv(path, signal)?;
    if fs_override > 0.0 {
        ts.fs = fs_override;
    }
    let result = run_beat_hrv_pipeline(&ts, &EcgPipelineConfig::default());
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn cmd_openbci_hrv(path: &Path, channel: &str, fs_override: f64) -> Result<()> {
    let mut ts = openbci_io::read_openbci_csv(path, channel)?;
    if fs_override > 0.0 {
        ts.fs = fs_override;
    }
    let result = run_beat_hrv_pipeline(&ts, &EcgPipelineConfig::default());
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn cmd_dataset_validate(spec_path: &Path, json: Option<&Path>) -> Result<()> {
    let spec_file = File::open(spec_path)
        .with_context(|| format!("failed to open spec {}", spec_path.display()))?;
    let spec: DatasetSpec =
        serde_json::from_reader(spec_file).context("failed to parse dataset spec")?;
    let repo_root = workspace_root();
    let mut results = Vec::new();
    match spec {
        DatasetSpec::Case(case) => {
            results.push(validate_case(&case, &repo_root, &CaseDefaults::default())?);
        }
        DatasetSpec::Suite(suite) => {
            let defaults = CaseDefaults {
                fs: suite.fs,
                interp_fs: suite.interp_fs,
                tolerance: suite.tolerance,
            };
            for case in &suite.cases {
                results.push(validate_case(case, &repo_root, &defaults)?);
            }
            println!(
                "suite {} validated ({} cases)",
                suite.name,
                suite.cases.len()
            );
        }
    }
    if let Some(path) = json {
        let file = File::create(path)
            .with_context(|| format!("failed to write report {}", path.display()))?;
        serde_json::to_writer_pretty(file, &results)?;
        println!("dataset report written to {}", path.display());
    } else {
        println!("{}", serde_json::to_string(&results)?);
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DatasetSpec {
    Suite(DatasetSuite),
    Case(DatasetCase),
}

#[derive(Deserialize)]
struct DatasetSuite {
    name: String,
    #[serde(default)]
    fs: Option<f64>,
    #[serde(default)]
    interp_fs: Option<f64>,
    #[serde(default)]
    tolerance: Option<f64>,
    cases: Vec<DatasetCase>,
}

#[derive(Default, Clone, Copy)]
struct CaseDefaults {
    fs: Option<f64>,
    interp_fs: Option<f64>,
    tolerance: Option<f64>,
}

#[derive(Serialize)]
struct TimeMetricsRecord {
    avnn: f64,
    sdnn: f64,
    rmssd: f64,
    pnn50: f64,
    tolerance: f64,
}

#[derive(Serialize)]
struct PsdMetricsRecord {
    lf: f64,
    hf: f64,
    vlf: f64,
    lf_hf: f64,
    total_power: f64,
    tolerance: f64,
}

#[derive(Serialize)]
struct DatasetResult {
    name: String,
    status: String,
    time: Option<TimeMetricsRecord>,
    psd: Option<PsdMetricsRecord>,
}

#[derive(Deserialize, Clone)]
struct DatasetCase {
    name: String,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    fs: Option<f64>,
    #[serde(default)]
    interp_fs: Option<f64>,
    #[serde(default)]
    tolerance: Option<f64>,
    #[serde(default)]
    wfdb_header: Option<String>,
    #[serde(default)]
    wfdb_lead: Option<usize>,
    #[serde(default)]
    annotations: Option<String>,
    #[serde(default)]
    bids_events: Option<String>,
    #[serde(default)]
    bitalino_input: Option<String>,
    #[serde(default)]
    bitalino_signal: Option<String>,
    #[serde(default)]
    openbci_input: Option<String>,
    #[serde(default)]
    openbci_channel: Option<String>,
    #[serde(default)]
    hrv_time: Option<HrvTimeSpec>,
    #[serde(default)]
    hrv_psd: Option<HrvPsdSpec>,
}

#[derive(Default, Deserialize, Clone)]
struct HrvTimeSpec {
    #[serde(default)]
    tolerance: Option<f64>,
    #[serde(default)]
    avnn: Option<f64>,
    #[serde(default)]
    sdnn: Option<f64>,
    #[serde(default)]
    rmssd: Option<f64>,
    #[serde(default)]
    pnn50: Option<f64>,
}

#[derive(Default, Deserialize, Clone)]
struct HrvPsdSpec {
    #[serde(default)]
    tolerance: Option<f64>,
    #[serde(default)]
    lf: Option<f64>,
    #[serde(default)]
    hf: Option<f64>,
    #[serde(default)]
    vlf: Option<f64>,
    #[serde(default)]
    lf_hf: Option<f64>,
    #[serde(default)]
    total_power: Option<f64>,
}

fn validate_case(
    case: &DatasetCase,
    repo_root: &Path,
    defaults: &CaseDefaults,
) -> Result<DatasetResult> {
    let tolerance = case.tolerance.or(defaults.tolerance).unwrap_or(0.5).abs();
    let interp_fs = case.interp_fs.or(defaults.interp_fs).unwrap_or(4.0);
    let rr = rr_series_from_case(case, repo_root, defaults)?;
    let time_metrics = hrv_time(&rr);
    let time_record = if let Some(spec) = &case.hrv_time {
        verify_time_metrics(&case.name, spec, &time_metrics, tolerance)?;
        Some(TimeMetricsRecord {
            avnn: time_metrics.avnn,
            sdnn: time_metrics.sdnn,
            rmssd: time_metrics.rmssd,
            pnn50: time_metrics.pnn50,
            tolerance: spec.tolerance.unwrap_or(tolerance).abs(),
        })
    } else {
        None
    };
    let psd_metrics = hrv_psd(&rr, interp_fs);
    let psd_record = if let Some(spec) = &case.hrv_psd {
        verify_psd_metrics(&case.name, spec, &psd_metrics, tolerance)?;
        Some(PsdMetricsRecord {
            lf: psd_metrics.lf,
            hf: psd_metrics.hf,
            vlf: psd_metrics.vlf,
            lf_hf: psd_metrics.lf_hf,
            total_power: psd_metrics.total_power,
            tolerance: spec.tolerance.unwrap_or(tolerance).abs(),
        })
    } else {
        None
    };
    println!("dataset {} validated", case.name);
    Ok(DatasetResult {
        name: case.name.clone(),
        status: "ok".into(),
        time: time_record,
        psd: psd_record,
    })
}

fn rr_series_from_case(
    case: &DatasetCase,
    repo_root: &Path,
    defaults: &CaseDefaults,
) -> Result<RRSeries> {
    if case.format.as_deref() == Some("rr") {
        let input = case
            .input
            .as_ref()
            .ok_or_else(|| anyhow!("dataset {} missing RR input path", case.name))?;
        let path = resolve_path(repo_root, input);
        let rr = text_io::read_f64_series(&path)?;
        return Ok(RRSeries { rr });
    }

    let events_fs = case.fs.or(defaults.fs);
    let annotation_path = case
        .annotations
        .as_ref()
        .map(|value| resolve_path(repo_root, value));
    let bids_events_path = case
        .bids_events
        .as_ref()
        .map(|value| resolve_path(repo_root, value));
    if annotation_path.is_some() || bids_events_path.is_some() {
        let fs = events_fs.ok_or_else(|| {
            anyhow!(
                "dataset {} requires fs when providing event annotations",
                case.name
            )
        })?;
        if annotation_path
            .as_deref()
            .map(|p| p.components().any(|comp| comp.as_os_str() == "run_bundle"))
            .unwrap_or(false)
        {
            ensure_run_bundle(repo_root)?;
        }
        if let Some(events) =
            load_annotation_events(annotation_path.as_deref(), bids_events_path.as_deref(), fs)?
        {
            return Ok(RRSeries::from_events(&events, fs));
        }
        anyhow::bail!("dataset {} produced no events from annotations", case.name);
    }

    if let Some(bitalino_input) = &case.bitalino_input {
        let signal = case.bitalino_signal.as_deref().unwrap_or("analog0");
        let path = resolve_path(repo_root, bitalino_input);
        let ts = bitalino_io::read_bitalino_csv(&path, signal)?;
        let result = run_beat_hrv_pipeline(&ts, &EcgPipelineConfig::default());
        return Ok(result.rr);
    }

    if let Some(openbci_input) = &case.openbci_input {
        let channel = case.openbci_channel.as_deref().unwrap_or("Ch1");
        let path = resolve_path(repo_root, openbci_input);
        let ts = openbci_io::read_openbci_csv(&path, channel)?;
        let result = run_beat_hrv_pipeline(&ts, &EcgPipelineConfig::default());
        return Ok(result.rr);
    }

    let ts = if let Some(header) = &case.wfdb_header {
        let lead = case.wfdb_lead.unwrap_or(0);
        let path = resolve_path(repo_root, header);
        wfdb_io::load_wfdb_lead(&path, lead)?
    } else {
        let input = case
            .input
            .as_ref()
            .ok_or_else(|| anyhow!("dataset {} missing time series input path", case.name))?;
        let path = resolve_path(repo_root, input);
        let data = text_io::read_f64_series(&path)?;
        let fs = events_fs.ok_or_else(|| {
            anyhow!(
                "dataset {} requires fs when providing raw samples",
                case.name
            )
        })?;
        TimeSeries { fs, data }
    };
    let result = run_beat_hrv_pipeline(&ts, &EcgPipelineConfig::default());
    Ok(result.rr)
}

fn verify_time_metrics(
    dataset: &str,
    spec: &HrvTimeSpec,
    computed: &HRVTime,
    tolerance: f64,
) -> Result<()> {
    let tol = spec.tolerance.unwrap_or(tolerance).abs();
    if let Some(expected) = spec.avnn {
        assert_within(dataset, "AVNN", expected, computed.avnn, tol)?;
    }
    if let Some(expected) = spec.sdnn {
        assert_within(dataset, "SDNN", expected, computed.sdnn, tol)?;
    }
    if let Some(expected) = spec.rmssd {
        assert_within(dataset, "RMSSD", expected, computed.rmssd, tol)?;
    }
    if let Some(expected) = spec.pnn50 {
        assert_within(dataset, "pNN50", expected, computed.pnn50, tol)?;
    }
    Ok(())
}

fn verify_psd_metrics(
    dataset: &str,
    spec: &HrvPsdSpec,
    computed: &HRVPsd,
    tolerance: f64,
) -> Result<()> {
    let tol = spec.tolerance.unwrap_or(tolerance).abs();
    if let Some(expected) = spec.lf {
        assert_within(dataset, "LF", expected, computed.lf, tol)?;
    }
    if let Some(expected) = spec.hf {
        assert_within(dataset, "HF", expected, computed.hf, tol)?;
    }
    if let Some(expected) = spec.vlf {
        assert_within(dataset, "VLF", expected, computed.vlf, tol)?;
    }
    if let Some(expected) = spec.lf_hf {
        assert_within(dataset, "LF/HF", expected, computed.lf_hf, tol)?;
    }
    if let Some(expected) = spec.total_power {
        assert_within(dataset, "Total Power", expected, computed.total_power, tol)?;
    }
    Ok(())
}

fn assert_within(dataset: &str, label: &str, expected: f64, actual: f64, tol: f64) -> Result<()> {
    if (actual - expected).abs() > tol {
        anyhow::bail!(
            "{} mismatch for {}: expected {}, got {}",
            label,
            dataset,
            expected,
            actual
        );
    }
    Ok(())
}

fn resolve_path(repo_root: &Path, input: &str) -> PathBuf {
    let path = Path::new(input);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn cmd_sqi(input: Option<&Path>, fs: f64) -> Result<()> {
    let data = read_samples(input)?;
    let ts = TimeSeries {
        fs: fs.max(1.0),
        data,
    };
    let events = detect_r_peaks(&ts, 0.3);
    let rr = RRSeries::from_events(&events, ts.fs);
    let sqi = evaluate_sqi(&ts, &rr);
    println!("{}", serde_json::to_string(&sqi)?);
    Ok(())
}

fn cmd_run_simulate(
    design: &Path,
    trials: &Path,
    sub: &str,
    ses: &str,
    run_id: &str,
    out: &Path,
) -> Result<()> {
    let design_spec = read_design(design)?;
    let trial_specs = read_trials(trials)?;
    let bundle = simulate_run(&design_spec, &trial_specs, sub, ses, run_id);
    fs::create_dir_all(out)?;
    write_events_tsv(&out.join("events.tsv"), &bundle.events)?;
    write_events_json(&out.join("events.json"))?;
    write_manifest(&out.join("run.json"), &bundle.manifest)?;
    println!("run bundle written to {}", out.display());
    Ok(())
}

fn draw_plotters_figure(path: &Path, fig: &Figure) -> Result<()> {
    let backend = BitMapBackend::new(path, (800, 480));
    let root = backend.into_drawing_area();
    root.fill(&WHITE)?;
    let x_values: Vec<f64> = fig
        .series
        .iter()
        .flat_map(|series| match series {
            Series::Line(line) => line.points.iter().map(|p| p[0]).collect::<Vec<_>>(),
        })
        .collect();
    let y_values: Vec<f64> = fig
        .series
        .iter()
        .flat_map(|series| match series {
            Series::Line(line) => line.points.iter().map(|p| p[1]).collect::<Vec<_>>(),
        })
        .collect();
    let x_min = *x_values
        .iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(&0.0);
    let x_max = *x_values
        .iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(&1.0);
    let y_min = *y_values
        .iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(&0.0);
    let y_max = *y_values
        .iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(&1.0);
    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .caption(
            fig.title.clone().unwrap_or_else(|| "Plot".into()),
            ("sans-serif", 24),
        )
        .x_label_area_size(30)
        .y_label_area_size(40)
        .build_cartesian_2d(x_min..x_max, y_min..y_max)?;
    chart.configure_mesh().draw()?;
    for series in &fig.series {
        match series {
            Series::Line(line) => {
                chart.draw_series(LineSeries::new(
                    line.points.iter().map(|p| (p[0], p[1])),
                    &RGBColor(
                        ((line.style.color.0 >> 16) & 0xFF) as u8,
                        ((line.style.color.0 >> 8) & 0xFF) as u8,
                        (line.style.color.0 & 0xFF) as u8,
                    ),
                ))?;
            }
        }
    }
    root.present()?;
    Ok(())
}

fn load_time_series(
    fs: f64,
    input: Option<&Path>,
    wfdb_header: Option<&Path>,
    wfdb_lead: usize,
    eeg_edf: Option<&Path>,
    eeg_channel: usize,
) -> Result<TimeSeries> {
    if let Some(header) = wfdb_header {
        wfdb_io::load_wfdb_lead(header, wfdb_lead)
    } else if let Some(edf) = eeg_edf {
        eeg_io::load_edf_channel(edf, eeg_channel)
    } else {
        let data = read_samples(input)?;
        Ok(TimeSeries { fs, data })
    }
}

fn load_annotation_events(
    annotations: Option<&Path>,
    bids_events: Option<&Path>,
    fs: f64,
) -> Result<Option<Events>> {
    if let Some(bids_path) = bids_events {
        return Ok(Some(eeg_io::load_bids_events_indices(bids_path, fs)?));
    }
    if let Some(path) = annotations {
        let events = if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if ext.eq_ignore_ascii_case("atr") {
                wfdb_io::load_wfdb_events(path)?
            } else {
                let indices = text_io::read_event_indices(path)?;
                Events::from_indices(indices)
            }
        } else {
            let indices = text_io::read_event_indices(path)?;
            Events::from_indices(indices)
        };
        return Ok(Some(events));
    }
    Ok(None)
}

fn cmd_pupil_normalize(path: &Path, format: EyeFormat, min_confidence: f32) -> Result<()> {
    let (timestamp_col, pupil_col, confidence_col, eye_col, delimiter) = format.columns();
    let samples = eye_io::read_eye_csv(
        path,
        timestamp_col,
        pupil_col,
        confidence_col,
        eye_col,
        delimiter,
    )
    .map_err(|e| anyhow!("{}", e))?;
    let filtered = eye_io::confidence_filter(&samples, min_confidence);
    for sample in filtered {
        println!("{}", serde_json::to_string(&sample)?);
    }
    Ok(())
}

fn cmd_ecg_find_rpeaks(
    fs: f64,
    min_rr_s: f64,
    input: Option<&Path>,
    wfdb_header: Option<&Path>,
    wfdb_lead: usize,
    eeg_edf: Option<&Path>,
    eeg_channel: usize,
) -> Result<()> {
    let ts = load_time_series(fs, input, wfdb_header, wfdb_lead, eeg_edf, eeg_channel)?;
    let events = detect_r_peaks(&ts, min_rr_s);
    let js = serde_json::to_string(&events)?;
    println!("{}", js);
    Ok(())
}

fn cmd_hrv_time(input: Option<&Path>) -> Result<()> {
    let rr = read_samples(input)?;
    let rr = RRSeries { rr };
    let m = hrv_time(&rr);
    let js = serde_json::to_string(&m)?;
    println!("{}", js);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_beat_hrv_pipeline(
    fs: f64,
    lowcut_hz: f64,
    highcut_hz: f64,
    integration_window_s: f64,
    min_rr_s: f64,
    threshold_scale: f64,
    search_back_s: f64,
    input: Option<&Path>,
    wfdb_header: Option<&Path>,
    wfdb_lead: usize,
    annotations: Option<&Path>,
    eeg_edf: Option<&Path>,
    eeg_channel: usize,
    bids_events: Option<&Path>,
) -> Result<()> {
    let ts = load_time_series(fs, input, wfdb_header, wfdb_lead, eeg_edf, eeg_channel)?;
    let cfg = EcgPipelineConfig {
        lowcut_hz,
        highcut_hz,
        integration_window_s,
        min_rr_s,
        threshold_scale,
        search_back_s,
    };
    let summary = if let Some(events) = load_annotation_events(annotations, bids_events, ts.fs)? {
        BeatHrvPipelineResult::from_events(&ts, events)
    } else {
        run_beat_hrv_pipeline(&ts, &cfg)
    };
    let js = serde_json::to_string(&summary)?;
    println!("{}", js);
    Ok(())
}
