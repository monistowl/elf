use crate::hrv_helpers::rr_histogram_figure;
use crate::run_loader::{RunEventFilter, RunManifest};
use crate::GuiTab;
use elf_lib::{
    io::eye as eye_io,
    metrics::{
        hrv::{hrv_nonlinear, hrv_psd, hrv_time, HRVNonlinear, HRVPsd, HRVTime},
        sqi::{evaluate_sqi, SQIResult},
    },
    plot::{decimate_points, figure_from_rr, Color, Figure, LineSeries, Series, Style},
    signal::{Events, RRSeries, TimeSeries},
};
use serde::Serialize;

const MAX_WAVEFORM_POINTS: usize = 2048;
const MAX_EEG_POINTS: usize = 2048;
const MAX_EYE_POINTS: usize = 1024;

#[derive(Default)]
struct DirtyFlags {
    waveform: bool,
    rr: bool,
    rr_figure: bool,
    hrv: bool,
    psd: bool,
    psd_figure: bool,
    nonlinear: bool,
    sqi: bool,
    rr_histogram: bool,
    eeg: bool,
    eye: bool,
}

impl DirtyFlags {
    fn mark_ecg(&mut self) {
        self.waveform = true;
        self.rr = true;
        self.rr_figure = true;
        self.hrv = true;
        self.psd = true;
        self.psd_figure = true;
        self.nonlinear = true;
        self.sqi = true;
        self.rr_histogram = true;
    }

    fn mark_events(&mut self) {
        self.rr = true;
        self.rr_figure = true;
        self.hrv = true;
        self.psd = true;
        self.psd_figure = true;
        self.nonlinear = true;
        self.sqi = true;
        self.rr_histogram = true;
    }
}

#[derive(Default)]
struct Snapshot {
    ecg: Option<TimeSeries>,
    events: Option<Events>,
    rr: Option<RRSeries>,
    hrv_time: Option<HRVTime>,
    hrv_psd: Option<HRVPsd>,
    hrv_nonlinear: Option<HRVNonlinear>,
    sqi: Option<SQIResult>,
    ecg_figure: Option<Figure>,
    rr_figure: Option<Figure>,
    psd_figure: Option<Figure>,
    rr_histogram: Option<Figure>,
    eeg: Option<TimeSeries>,
    eeg_events: Vec<f64>,
    eeg_figure: Option<Figure>,
    eye_samples: Vec<eye_io::PupilSample>,
    eye_filtered: Vec<eye_io::PupilSample>,
    eye_figure: Option<Figure>,
}

pub struct Store {
    snapshot: Snapshot,
    dirty: DirtyFlags,
    eye_threshold: f32,
    psd_interp_fs: f64,
    run_bundle_state: Option<RunBundleState>,
}

#[derive(Debug, Serialize)]
pub struct HrvSnapshot {
    pub events: Option<Events>,
    pub rr: Option<RRSeries>,
    pub fs: Option<f64>,
    pub hrv_time: Option<HRVTime>,
    pub hrv_psd: Option<HRVPsd>,
    pub hrv_nonlinear: Option<HRVNonlinear>,
    pub psd_interp_fs: f64,
    pub run_bundle_state: Option<RunBundleState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunBundleState {
    manifest: RunManifest,
    filter: RunEventFilter,
    bundle_path: Option<String>,
}

impl RunBundleState {
    pub fn new(manifest: RunManifest, filter: RunEventFilter, bundle_path: Option<String>) -> Self {
        Self {
            manifest,
            filter,
            bundle_path,
        }
    }

    pub fn manifest(&self) -> &RunManifest {
        &self.manifest
    }

    pub fn filter(&self) -> &RunEventFilter {
        &self.filter
    }

    pub fn bundle_path(&self) -> Option<&str> {
        self.bundle_path.as_deref()
    }
}

impl Default for Store {
    fn default() -> Self {
        Self {
            snapshot: Snapshot::default(),
            dirty: DirtyFlags {
                waveform: true,
                rr: true,
                rr_figure: true,
                hrv: true,
                psd: true,
                psd_figure: true,
                nonlinear: true,
                sqi: true,
                rr_histogram: true,
                eeg: true,
                eye: true,
            },
            eye_threshold: 0.5,
            psd_interp_fs: 4.0,
            run_bundle_state: None,
        }
    }
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_ecg(&mut self, ts: TimeSeries) {
        self.snapshot.ecg = Some(ts);
        self.dirty.mark_ecg();
        self.snapshot.rr = None;
    }

    pub fn set_events(&mut self, events: Events) {
        self.snapshot.events = Some(events);
        self.dirty.mark_events();
        self.snapshot.rr = None;
    }

    pub fn apply_stream_metrics(
        &mut self,
        rr: RRSeries,
        hrv_time: HRVTime,
        hrv_psd: HRVPsd,
        hrv_nonlinear: HRVNonlinear,
    ) {
        self.snapshot.rr = Some(rr);
        self.snapshot.hrv_time = Some(hrv_time);
        self.snapshot.hrv_psd = Some(hrv_psd);
        self.snapshot.hrv_nonlinear = Some(hrv_nonlinear);
        self.dirty.rr = false;
        self.dirty.rr_figure = true;
        self.dirty.hrv = false;
        self.dirty.psd = false;
        self.dirty.psd_figure = true;
        self.dirty.nonlinear = false;
        self.dirty.sqi = true;
    }

    pub fn set_eeg(&mut self, ts: TimeSeries) {
        self.snapshot.eeg = Some(ts);
        self.dirty.eeg = true;
    }

    pub fn set_eeg_events(&mut self, events: Vec<f64>) {
        self.snapshot.eeg_events = events;
    }

    pub fn set_eye_samples(&mut self, samples: Vec<eye_io::PupilSample>) {
        self.snapshot.eye_samples = samples;
        self.update_eye_filter();
    }

    pub fn set_eye_threshold(&mut self, threshold: f32) {
        self.eye_threshold = threshold;
        self.update_eye_filter();
    }

    fn update_eye_filter(&mut self) {
        let filtered: Vec<_> = self
            .snapshot
            .eye_samples
            .iter()
            .filter(|sample| sample.confidence.unwrap_or(1.0) >= self.eye_threshold)
            .cloned()
            .collect();
        self.snapshot.eye_filtered = filtered;
        self.dirty.eye = true;
    }

    pub fn prepare(&mut self, tab: GuiTab) {
        match tab {
            GuiTab::Hrv => {
                self.ensure_waveform_figure();
                self.ensure_rr_series();
                self.ensure_rr_figure();
                self.ensure_hrv_time();
                self.ensure_psd();
                self.ensure_psd_figure();
                self.ensure_nonlinear();
                self.ensure_sqi();
                self.ensure_rr_histogram();
            }
            GuiTab::Eeg => self.ensure_eeg_figure(),
            GuiTab::Eye => self.ensure_eye_figure(),
            _ => {}
        }
    }

    pub fn ecg(&self) -> Option<&TimeSeries> {
        self.snapshot.ecg.as_ref()
    }

    pub fn ecg_len(&self) -> usize {
        self.snapshot.ecg.as_ref().map(|ts| ts.len()).unwrap_or(0)
    }

    pub fn events_len(&self) -> usize {
        self.snapshot
            .events
            .as_ref()
            .map(|events| events.indices.len())
            .unwrap_or(0)
    }

    pub fn event_seconds(&self) -> Vec<f64> {
        if let (Some(events), Some(ts)) =
            (self.snapshot.events.as_ref(), self.snapshot.ecg.as_ref())
        {
            let fs = ts.fs.max(1.0);
            events.indices.iter().map(|idx| *idx as f64 / fs).collect()
        } else {
            Vec::new()
        }
    }

    pub fn rr_series(&self) -> Option<&RRSeries> {
        self.snapshot.rr.as_ref()
    }

    pub fn hrv_time(&self) -> Option<&HRVTime> {
        self.snapshot.hrv_time.as_ref()
    }

    pub fn hrv_psd(&self) -> Option<&HRVPsd> {
        self.snapshot.hrv_psd.as_ref()
    }

    pub fn hrv_nonlinear(&self) -> Option<&HRVNonlinear> {
        self.snapshot.hrv_nonlinear.as_ref()
    }

    pub fn ecg_figure(&self) -> Option<&Figure> {
        self.snapshot.ecg_figure.as_ref()
    }

    #[allow(dead_code)]
    pub fn rr_figure(&self) -> Option<&Figure> {
        self.snapshot.rr_figure.as_ref()
    }

    pub fn rr_histogram(&self) -> Option<&Figure> {
        self.snapshot.rr_histogram.as_ref()
    }

    pub fn psd_figure(&self) -> Option<&Figure> {
        self.snapshot.psd_figure.as_ref()
    }

    pub fn psd_interp_fs(&self) -> f64 {
        self.psd_interp_fs
    }

    pub fn set_psd_interp_fs(&mut self, interp_fs: f64) {
        if (self.psd_interp_fs - interp_fs).abs() < f64::EPSILON {
            return;
        }
        self.psd_interp_fs = interp_fs;
        self.dirty.psd = true;
        self.dirty.psd_figure = true;
    }

    pub fn events(&self) -> Option<&Events> {
        self.snapshot.events.as_ref()
    }

    pub fn hrv_snapshot(&self) -> HrvSnapshot {
        HrvSnapshot {
            events: self.snapshot.events.clone(),
            rr: self.snapshot.rr.clone(),
            fs: self.snapshot.ecg.as_ref().map(|ts| ts.fs),
            hrv_time: self.snapshot.hrv_time.clone(),
            hrv_psd: self.snapshot.hrv_psd.clone(),
            hrv_nonlinear: self.snapshot.hrv_nonlinear.clone(),
            psd_interp_fs: self.psd_interp_fs,
            run_bundle_state: self.run_bundle_state.clone(),
        }
    }

    pub fn set_run_bundle_state(&mut self, state: Option<RunBundleState>) {
        self.run_bundle_state = state;
    }

    pub fn eye_figure(&self) -> Option<&Figure> {
        self.snapshot.eye_figure.as_ref()
    }

    pub fn eeg_figure(&self) -> Option<&Figure> {
        self.snapshot.eeg_figure.as_ref()
    }

    pub fn eeg_events(&self) -> &[f64] {
        &self.snapshot.eeg_events
    }

    #[allow(dead_code)]
    pub fn eeg_fs(&self) -> Option<f64> {
        self.snapshot.eeg.as_ref().map(|ts| ts.fs)
    }

    pub fn eeg_sample_count(&self) -> usize {
        self.snapshot.eeg.as_ref().map(|ts| ts.len()).unwrap_or(0)
    }

    pub fn eye_filtered(&self) -> &[eye_io::PupilSample] {
        &self.snapshot.eye_filtered
    }

    pub fn eye_total(&self) -> usize {
        self.snapshot.eye_samples.len()
    }

    pub fn sqi(&self) -> Option<&SQIResult> {
        self.snapshot.sqi.as_ref()
    }

    fn ensure_waveform_figure(&mut self) {
        if !self.dirty.waveform {
            return;
        }
        let figure =
            self.snapshot.ecg.as_ref().map(|ts| {
                figure_from_timeseries("ECG waveform", ts, MAX_WAVEFORM_POINTS, 0xFF3333)
            });
        self.snapshot.ecg_figure = figure;
        self.dirty.waveform = false;
    }

    fn ensure_rr_series(&mut self) -> Option<&RRSeries> {
        if self.snapshot.rr.is_none() {
            if let (Some(events), Some(ts)) =
                (self.snapshot.events.as_ref(), self.snapshot.ecg.as_ref())
            {
                if events.indices.len() > 1 {
                    let rr = RRSeries::from_events(events, ts.fs);
                    self.snapshot.rr = Some(rr);
                    self.dirty.rr_figure = true;
                }
            }
        }
        self.snapshot.rr.as_ref()
    }

    fn ensure_rr_figure(&mut self) {
        if !self.dirty.rr_figure {
            return;
        }
        let figure = self.snapshot.rr.as_ref().map(|rr| figure_from_rr(rr));
        self.snapshot.rr_figure = figure;
        self.dirty.rr_figure = false;
    }

    fn ensure_rr_histogram(&mut self) {
        if !self.dirty.rr_histogram {
            return;
        }
        if let Some(rr) = self.snapshot.rr.as_ref() {
            self.snapshot.rr_histogram = rr_histogram_figure(rr, 12);
        } else {
            self.snapshot.rr_histogram = None;
        }
        self.dirty.rr_histogram = false;
    }

    fn ensure_hrv_time(&mut self) {
        if !self.dirty.hrv {
            return;
        }
        if let Some(rr) = self.ensure_rr_series() {
            self.snapshot.hrv_time = Some(hrv_time(rr));
        } else {
            self.snapshot.hrv_time = None;
        }
        self.dirty.hrv = false;
    }

    fn ensure_psd(&mut self) {
        if !self.dirty.psd {
            return;
        }
        let interp_fs = self.psd_interp_fs;
        if let Some(rr) = self.ensure_rr_series() {
            self.snapshot.hrv_psd = Some(hrv_psd(rr, interp_fs));
        } else {
            self.snapshot.hrv_psd = None;
        }
        self.dirty.psd = false;
        self.dirty.psd_figure = true;
    }

    fn ensure_psd_figure(&mut self) {
        if !self.dirty.psd_figure {
            return;
        }
        let figure = self.snapshot.hrv_psd.as_ref().map(|psd| {
            figure_from_points(Some("PSD".to_string()), "PSD", psd.points.clone(), 0x0077FF)
        });
        self.snapshot.psd_figure = figure;
        self.dirty.psd_figure = false;
    }

    fn ensure_sqi(&mut self) {
        if !self.dirty.sqi {
            return;
        }
        if let (Some(ts), Some(rr)) = (self.snapshot.ecg.as_ref(), self.snapshot.rr.as_ref()) {
            self.snapshot.sqi = Some(evaluate_sqi(ts, rr));
        } else {
            self.snapshot.sqi = None;
        }
        self.dirty.sqi = false;
    }

    fn ensure_nonlinear(&mut self) {
        if !self.dirty.nonlinear {
            return;
        }
        if let Some(rr) = self.ensure_rr_series() {
            self.snapshot.hrv_nonlinear = Some(hrv_nonlinear(rr));
        } else {
            self.snapshot.hrv_nonlinear = None;
        }
        self.dirty.nonlinear = false;
    }

    fn ensure_eeg_figure(&mut self) {
        if !self.dirty.eeg {
            return;
        }
        let figure = self
            .snapshot
            .eeg
            .as_ref()
            .map(|ts| figure_from_timeseries("EEG trace", ts, MAX_EEG_POINTS, 0x33CCFF));
        self.snapshot.eeg_figure = figure;
        self.dirty.eeg = false;
    }

    fn ensure_eye_figure(&mut self) {
        if !self.dirty.eye {
            return;
        }
        let points: Vec<[f64; 2]> = self
            .snapshot
            .eye_filtered
            .iter()
            .filter_map(|sample| sample.pupil_mm.map(|p| [sample.timestamp, p as f64]))
            .collect();
        let figure = if points.is_empty() {
            None
        } else {
            let decimated = decimate_points(&points, MAX_EYE_POINTS);
            Some(figure_from_points(
                Some("Pupil size".to_string()),
                "Pupil",
                decimated,
                0xFFCC33,
            ))
        };
        self.snapshot.eye_figure = figure;
        self.dirty.eye = false;
    }
}

fn sample_points(series: &TimeSeries, max_points: usize) -> Vec<[f64; 2]> {
    let dt = 1.0 / series.fs.max(1.0);
    let coords: Vec<[f64; 2]> = series
        .data
        .iter()
        .enumerate()
        .map(|(i, &value)| [i as f64 * dt, value])
        .collect();
    decimate_points(&coords, max_points)
}

fn figure_from_timeseries(
    title: &str,
    series: &TimeSeries,
    max_points: usize,
    color: u32,
) -> Figure {
    let points = sample_points(series, max_points);
    figure_from_points(Some(title.to_string()), title, points, color)
}

fn figure_from_points(
    title: Option<String>,
    name: &str,
    points: Vec<[f64; 2]>,
    color: u32,
) -> Figure {
    let mut fig = Figure::new(title);
    fig.add_series(Series::Line(LineSeries {
        name: name.into(),
        points,
        style: Style {
            width: 1.4,
            dash: None,
            color: Color(color),
        },
    }));
    fig
}
