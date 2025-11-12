use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use elf_lib::{
    detectors::ecg::{
        detect_r_peaks, run_beat_hrv_pipeline, BeatHrvPipelineResult, EcgPipelineConfig,
    },
    io::{eeg as eeg_io, eye as eye_io, text as text_io, wfdb as wfdb_io},
    metrics::hrv::{hrv_nonlinear, hrv_psd, hrv_time},
    plot::{figure_from_rr, Figure, Series},
    signal::{Events, RRSeries, TimeSeries},
};
use plotters::prelude::*;
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

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
        #[arg(long, default_value_t = 0.3)]
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
        #[arg(long, default_value_t = 0.280)]
        min_rr_s: f64,
        #[arg(long, default_value_t = 0.3)]
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
