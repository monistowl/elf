use crossbeam_channel::{bounded, Sender};
use eframe::{egui, egui::ViewportBuilder};
use egui::{Color32, Margin, ScrollArea};
use egui_plot::{Line, Plot, VLine};
use elf_keys::KeyEntry;
use elf_lib::detectors::ecg::{run_beat_hrv_pipeline, EcgPipelineConfig};
use elf_lib::io::{eeg as eeg_io, eye as eye_io, text as text_io, wfdb as wfdb_io};
use elf_lib::plot::{Figure, Series, Style};
use elf_lib::signal::{Events, TimeSeries};
use rfd::FileDialog;
use serde_json;
use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ELF Dashboard (MVP)",
        native_options,
        Box::new(|_cc| Ok(Box::<ElfApp>::default())),
    )
}

#[derive(Copy, Clone, PartialEq)]
enum GuiTab {
    Landing,
    Hrv,
    Eeg,
    Eye,
    Settings,
}

impl GuiTab {
    fn title(&self) -> &'static str {
        match self {
            GuiTab::Landing => "Landing",
            GuiTab::Hrv => "ECG / HRV",
            GuiTab::Eeg => "EEG",
            GuiTab::Eye => "Eye",
            GuiTab::Settings => "Security",
        }
    }

    fn all() -> [GuiTab; 5] {
        [
            GuiTab::Landing,
            GuiTab::Hrv,
            GuiTab::Eeg,
            GuiTab::Eye,
            GuiTab::Settings,
        ]
    }
}

mod hrv_helpers;
mod router;
mod run_loader;
mod store;

use hrv_helpers::{average_rr, heart_rate_from_rr};
use router::{LslStatus, RecordingStatus, StreamCommand, StreamingStateRouter};
use run_loader::{
    events_from_records, load_events_with_filter, load_manifest, RunEventFilter, RunEventRecord,
    RunManifest,
};
use std::collections::HashMap;
use store::{RunBundleState, Store};

enum HrvExportOutcome {
    Exported(PathBuf),
    Cancelled,
    NoData,
}

#[derive(Copy, Clone, PartialEq)]
enum EyeLayout {
    PupilLabs,
    Tobii,
}

impl EyeLayout {
    fn label(&self) -> &'static str {
        match self {
            EyeLayout::PupilLabs => "Pupil Labs CSV",
            EyeLayout::Tobii => "Tobii TSV",
        }
    }

    fn parameters(
        &self,
    ) -> (
        &'static str,
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        u8,
    ) {
        match self {
            EyeLayout::PupilLabs => (
                "timestamp",
                "diameter",
                Some("confidence"),
                Some("eye"),
                b',',
            ),
            EyeLayout::Tobii => (
                "system_time_stamp",
                "pupil_diameter_2d",
                Some("confidence"),
                Some("eye"),
                b'\t',
            ),
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum BatchCommand {
    EcgFindRpeaks,
    BeatHrvPipeline,
}

impl BatchCommand {
    fn label(&self) -> &'static str {
        match self {
            BatchCommand::EcgFindRpeaks => "ecg-find-rpeaks",
            BatchCommand::BeatHrvPipeline => "beat-hrv-pipeline",
        }
    }

    fn title(&self) -> &'static str {
        match self {
            BatchCommand::EcgFindRpeaks => "Detect R-peaks",
            BatchCommand::BeatHrvPipeline => "Full HRV pipeline",
        }
    }
}

#[derive(Clone)]
struct LandingBatch {
    inputs: Vec<PathBuf>,
    dest: Option<PathBuf>,
    command: BatchCommand,
    fs: String,
    min_rr: String,
    threshold_scale: f64,
    use_annotations: bool,
    annotations: Option<PathBuf>,
    use_bids: bool,
    bids_events: Option<PathBuf>,
}

impl Default for LandingBatch {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            dest: None,
            command: BatchCommand::BeatHrvPipeline,
            fs: "250".to_string(),
            min_rr: "0.12".to_string(),
            threshold_scale: 0.6,
            use_annotations: false,
            annotations: None,
            use_bids: false,
            bids_events: None,
        }
    }
}

struct BatchFeedback {
    file: PathBuf,
    message: String,
    success: bool,
}

const SYNTHETIC_RR: [f64; 12] = [
    0.82, 0.78, 0.8, 0.79, 0.83, 0.77, 0.84, 0.88, 0.86, 0.81, 0.79, 0.82,
];
const STREAM_CHUNK_SIZE: usize = 4;
const STREAM_INTERVAL_MS: u64 = 450;

fn synthetic_event_indices(fs: f64) -> Vec<usize> {
    let mut events = Vec::with_capacity(SYNTHETIC_RR.len() + 1);
    let mut t = 0.0;
    events.push(0);
    for &rr in SYNTHETIC_RR.iter() {
        t += rr;
        events.push((t * fs).round() as usize);
    }
    events
}

struct StreamingSimulator {
    stop_tx: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl StreamingSimulator {
    fn start(cmd_sender: Sender<StreamCommand>, fs: f64) -> Self {
        let (stop_tx, stop_rx) = bounded(1);
        let handle = std::thread::spawn(move || {
            let indices = synthetic_event_indices(fs);
            for chunk in indices.chunks(STREAM_CHUNK_SIZE) {
                if chunk.is_empty() {
                    continue;
                }
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                let events = Events::from_indices(chunk.to_vec());
                if cmd_sender
                    .send(StreamCommand::IngestEvents(events, fs))
                    .is_err()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(STREAM_INTERVAL_MS));
            }
        });
        Self {
            stop_tx,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn synthetic_recording_path() -> PathBuf {
    workspace_root().join("test_data/synthetic_recording_a.txt")
}

struct ElfApp {
    store: StreamingStateRouter,
    raw_path: Option<String>,
    annotation_path: Option<String>,
    fs: f64,
    psd_interp_fs: f64,
    status: String,
    active_tab: GuiTab,
    landing_batch: LandingBatch,
    batch_receiver: Option<mpsc::Receiver<BatchFeedback>>,
    batch_running: bool,
    batch_summary: Vec<BatchFeedback>,
    // EEG tab state
    eeg_channel: usize,
    eeg_event_source: Option<String>,
    eeg_path: Option<String>,
    eeg_status: String,
    // Eye tab state
    eye_path: Option<String>,
    eye_min_conf: f32,
    eye_status: String,
    eye_layout: EyeLayout,
    stream_simulator: Option<StreamingSimulator>,
    run_bundle_path: Option<String>,
    run_manifest: Option<RunManifest>,
    run_bundle_event_filter: String,
    run_bundle_onset_column: String,
    run_bundle_event_type_column: String,
    run_bundle_duration_column: String,
    run_bundle_label_column: String,
    run_bundle_event_records: Vec<RunEventRecord>,
    run_bundle_event_summary: String,
    lsl_query: String,
    lsl_chunk_samples: usize,
    lsl_fs_hint: f64,
    lsl_selected_stream: Option<usize>,
    lsl_selected_channel: usize,
    key_list: Vec<KeyEntry>,
    keys_loaded: bool,
    key_status: String,
    key_name: String,
    key_validity_days: u32,
    import_name: String,
    import_cert: Option<PathBuf>,
    import_key: Option<PathBuf>,
}

impl Default for ElfApp {
    fn default() -> Self {
        Self {
            store: StreamingStateRouter::new(Store::new()),
            raw_path: None,
            annotation_path: None,
            fs: 250.0,
            psd_interp_fs: 4.0,
            status: "No recording loaded".into(),
            active_tab: GuiTab::Landing,
            landing_batch: LandingBatch::default(),
            batch_receiver: None,
            batch_running: false,
            batch_summary: Vec::new(),
            eeg_channel: 0,
            eeg_event_source: None,
            eeg_path: None,
            eeg_status: "No EEG loaded".into(),
            eye_path: None,
            eye_min_conf: 0.5,
            eye_status: "No eye data".into(),
            eye_layout: EyeLayout::PupilLabs,
            stream_simulator: None,
            run_bundle_path: None,
            run_manifest: None,
            run_bundle_event_filter: "stim".into(),
            run_bundle_onset_column: "onset".into(),
            run_bundle_event_type_column: "event_type".into(),
            run_bundle_duration_column: "duration".into(),
            run_bundle_label_column: "event_type".into(),
            run_bundle_event_records: Vec::new(),
            run_bundle_event_summary: String::new(),
            lsl_query: "ECG".into(),
            lsl_chunk_samples: 256,
            lsl_fs_hint: 250.0,
            lsl_selected_stream: None,
            lsl_selected_channel: 0,
            key_list: Vec::new(),
            keys_loaded: false,
            key_status: "Key manager idle".into(),
            key_name: String::new(),
            key_validity_days: 365,
            import_name: String::new(),
            import_cert: None,
            import_key: None,
        }
    }
}

impl ElfApp {
    fn load_raw(&mut self, path: &Path) -> Result<(), String> {
        let (ts, status_label) = if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if ext == "hea" {
                let ts = wfdb_io::load_wfdb_lead(path, 0).map_err(|e| e.to_string())?;
                let label = format!("Loaded WFDB record {}", path.display());
                (ts, label)
            } else if ext == "dat" {
                let header = path.with_extension("hea");
                if !header.exists() {
                    return Err(format!("WFDB header not found for {}", path.display()));
                }
                let ts = wfdb_io::load_wfdb_lead(&header, 0).map_err(|e| e.to_string())?;
                let label = format!(
                    "Loaded WFDB record {} (via {})",
                    header.display(),
                    path.display()
                );
                (ts, label)
            } else {
                let samples = text_io::read_f64_series(path).map_err(|e| e.to_string())?;
                let ts = TimeSeries {
                    fs: self.fs,
                    data: samples,
                };
                (ts, format!("Loaded raw ECG from {}", path.display()))
            }
        } else {
            let samples = text_io::read_f64_series(path).map_err(|e| e.to_string())?;
            let ts = TimeSeries {
                fs: self.fs,
                data: samples,
            };
            (ts, format!("Loaded raw ECG from {}", path.display()))
        };

        self.fs = ts.fs.max(1.0);
        let len = ts.data.len();
        self.store.set_ecg(ts);
        self.raw_path = Some(path.display().to_string());
        self.status = format!("{} ({} samples @ {:.1} Hz)", status_label, len, self.fs);
        Ok(())
    }

    fn load_annotations(&mut self, path: &Path) -> Result<(), String> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_ascii_lowercase());

        let events = match extension.as_deref() {
            Some("atr") => wfdb_io::load_wfdb_events(path).map_err(|e| e.to_string())?,
            Some("tsv") => {
                eeg_io::load_bids_events_indices(path, self.fs).map_err(|e| e.to_string())?
            }
            _ => {
                let indices = text_io::read_event_indices(path).map_err(|e| e.to_string())?;
                Events::from_indices(indices)
            }
        };

        let fs = self.store.ecg().map(|ts| ts.fs).unwrap_or(self.fs).max(1.0);
        self.store.submit_events(events, fs);
        self.annotation_path = Some(path.display().to_string());
        self.status = format!("Loaded {} annotations", self.store.events_len());
        Ok(())
    }

    fn run_detection(&mut self) -> Result<(), String> {
        let ts = self
            .store
            .ecg()
            .ok_or_else(|| "Load raw ECG before running detection".to_string())?;
        let cfg = EcgPipelineConfig::default();
        let result = run_beat_hrv_pipeline(ts, &cfg);
        let beats = result.events.indices.len();
        self.store.submit_events(result.events, ts.fs.max(1.0));
        self.status = format!("Detected {} beats", beats);
        Ok(())
    }

    fn toggle_streaming(&mut self) {
        if let Some(sim) = self.stream_simulator.take() {
            sim.stop();
            self.status = "Stopped synthetic stream".into();
        } else {
            let fs = self.fs.max(1.0);
            self.stream_simulator =
                Some(StreamingSimulator::start(self.store.command_sender(), fs));
            self.status = "Streaming synthetic beats".into();
        }
    }

    fn process_synthetic_ecg(&mut self) -> Result<(), String> {
        let path = synthetic_recording_path();
        let samples = text_io::read_f64_series(&path).map_err(|e| e.to_string())?;
        let ts = TimeSeries {
            fs: self.fs.max(1.0),
            data: samples,
        };
        self.store.submit_ecg(ts);
        self.status = format!("Queued synthetic ECG ({})", path.display());
        Ok(())
    }

    fn refresh_lsl_streams(&mut self) {
        match self.store.discover_lsl_streams(self.lsl_query.clone()) {
            Ok(_) => {
                self.status = format!("Refreshing LSL streams for '{}'", self.lsl_query);
                self.lsl_selected_stream = None;
            }
            Err(err) => {
                self.status = format!("Stream discovery failed: {err}");
            }
        }
    }

    fn toggle_lsl_stream(&mut self) {
        if self.store.is_lsl_streaming() {
            self.store.stop_lsl_stream();
            self.status = "Stopped LSL inlet".into();
            return;
        }
        let (index, stream) = {
            let streams = self.store.lsl_streams();
            if streams.is_empty() {
                self.status = "No LSL streams available; refresh to discover".into();
                return;
            }
            let idx = match self.lsl_selected_stream {
                Some(idx) if idx < streams.len() => idx,
                _ => 0,
            };
            let record = streams
                .get(idx)
                .cloned()
                .unwrap_or_else(|| streams[0].clone());
            (idx, record)
        };
        self.lsl_selected_stream = Some(index);
        let chunk = self.lsl_chunk_samples.max(1);
        let fs_hint = if self.lsl_fs_hint.is_finite() && self.lsl_fs_hint > 0.0 {
            Some(self.lsl_fs_hint)
        } else {
            None
        };
        let available_channels = stream.channels.max(1) as usize;
        if self.lsl_selected_channel >= available_channels {
            self.lsl_selected_channel = available_channels.saturating_sub(1);
        }
        let channel = self.lsl_selected_channel;
        match self.store.start_lsl_stream(
            stream.query.clone(),
            stream.source_id.clone(),
            channel,
            chunk,
            fs_hint,
        ) {
            Ok(_) => {
                self.status = format!(
                    "Connecting to {} ({}) channel {}",
                    stream.name, stream.source_id, channel
                );
            }
            Err(err) => {
                self.status = format!("LSL connect failed: {err}");
            }
        }
    }

    fn start_recording_dialog(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter("Parquet", &["parquet"])
            .set_file_name("recording.parquet")
            .save_file()
        {
            let fs = self.store.ecg().map(|ts| ts.fs).unwrap_or(self.fs).max(1.0);
            match self.store.start_recording(path.clone(), fs) {
                Ok(_) => {
                    self.status = format!("Recording to {}", path.display());
                }
                Err(err) => {
                    self.status = format!("Recording failed: {err}");
                }
            }
        }
    }

    fn stop_recording(&mut self) {
        match self.store.stop_recording() {
            Ok(_) => {
                self.status = "Stopped recording".into();
            }
            Err(err) => {
                self.status = format!("Recording stop failed: {err}");
            }
        }
    }

    fn load_eeg_trace(&mut self, path: &Path, channel: usize) -> Result<(), String> {
        let ts = eeg_io::load_edf_channel(path, channel).map_err(|e| e.to_string())?;
        let len = ts.data.len();
        let rate = ts.fs;
        self.store.set_eeg(ts);
        self.eeg_channel = channel;
        self.eeg_path = Some(path.display().to_string());
        self.eeg_status = format!("Loaded {} samples at {:.1} Hz", len, rate);
        Ok(())
    }

    fn load_eeg_events(&mut self, path: &Path) -> Result<(), String> {
        let events = eeg_io::load_bids_events(path).map_err(|e| e.to_string())?;
        let onsets = events.into_iter().map(|ev| ev.onset).collect();
        self.store.set_eeg_events(onsets);
        self.eeg_event_source = Some(path.display().to_string());
        self.eeg_status = format!("Loaded {} events", self.store.eeg_events().len());
        Ok(())
    }

    fn load_eye_csv(&mut self, path: &Path) -> Result<(), String> {
        let (timestamp_col, pupil_col, confidence_col, eye_col, delimiter) =
            self.eye_layout.parameters();
        let samples = eye_io::read_eye_csv(
            path,
            timestamp_col,
            pupil_col,
            confidence_col,
            eye_col,
            delimiter,
        )
        .map_err(|e| e.to_string())?;
        self.store.set_eye_samples(samples);
        self.store.set_eye_threshold(self.eye_min_conf);
        self.eye_path = Some(path.display().to_string());
        self.eye_status = format!(
            "Loaded {} eye samples ({})",
            self.store.eye_total(),
            self.eye_layout.label()
        );
        Ok(())
    }

    fn try_load_run_bundle(&mut self, path: &Path) -> Result<(), String> {
        let events_path = path.join("events.tsv");
        let manifest_path = path.join("run.json");
        self.store.set_run_bundle_state(None);
        let filter = self.build_run_event_filter();
        let records = load_events_with_filter(&events_path, &filter)
            .map_err(|e| format!("Run events load failed: {}", e))?;
        if records.is_empty() {
            return Err("No run bundle events matched the configured filter".into());
        }
        let fs = self.store.ecg().map(|ts| ts.fs).unwrap_or(self.fs).max(1.0);
        let events = events_from_records(&records, fs);
        self.store.submit_events(events, fs);
        self.run_bundle_event_records = records;
        self.update_run_event_summary();
        let manifest = load_manifest(&manifest_path)
            .map_err(|e| format!("Run manifest load failed: {}", e))?;
        self.store.set_run_bundle_state(Some(RunBundleState::new(
            manifest.clone(),
            filter,
            Some(path.display().to_string()),
        )));
        self.run_bundle_path = Some(path.display().to_string());
        self.run_manifest = Some(manifest);
        self.status = format!(
            "Loaded run bundle from {} ({} stimuli)",
            path.display(),
            self.run_bundle_event_records.len()
        );
        Ok(())
    }

    fn export_hrv_snapshot(&mut self) -> Result<HrvExportOutcome, String> {
        let snapshot = self.store.hrv_snapshot();
        if snapshot.rr.is_none() && snapshot.events.is_none() {
            return Ok(HrvExportOutcome::NoData);
        }
        if let Some(path) = FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("hrv_snapshot.json")
            .save_file()
        {
            let file = File::create(&path).map_err(|e| e.to_string())?;
            serde_json::to_writer_pretty(file, &snapshot).map_err(|e| e.to_string())?;
            return Ok(HrvExportOutcome::Exported(path));
        }
        Ok(HrvExportOutcome::Cancelled)
    }

    fn build_run_event_filter(&self) -> RunEventFilter {
        let mut filter = RunEventFilter::default();
        filter.onset_column = self.run_bundle_onset_column.clone();
        filter.event_type_column = self.run_bundle_event_type_column.clone();
        let duration = self.run_bundle_duration_column.trim();
        filter.duration_column = if duration.is_empty() {
            None
        } else {
            Some(duration.to_string())
        };
        let label = self.run_bundle_label_column.trim();
        filter.label_column = if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        };
        let types: Vec<String> = self
            .run_bundle_event_filter
            .split(',')
            .map(|segment| segment.trim())
            .filter(|segment| !segment.is_empty())
            .map(|segment| segment.to_string())
            .collect();
        if types.is_empty() {
            filter.allowed_event_types.clear();
        } else {
            filter.allowed_event_types = types;
        }
        filter
    }

    fn update_run_event_summary(&mut self) {
        if self.run_bundle_event_records.is_empty() {
            self.run_bundle_event_summary.clear();
            return;
        }
        let mut counts = HashMap::new();
        let mut label_examples = HashMap::new();
        for record in &self.run_bundle_event_records {
            *counts.entry(record.event_type.clone()).or_insert(0) += 1;
            if let Some(label) = record.label.as_deref() {
                label_examples
                    .entry(record.event_type.clone())
                    .or_insert_with(|| label.to_string());
            }
        }
        let summary = counts
            .into_iter()
            .map(|(event_type, count)| {
                if let Some(example) = label_examples.get(&event_type) {
                    format!("{}:{} ({})", event_type, count, example)
                } else {
                    format!("{}:{}", event_type, count)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        self.run_bundle_event_summary =
            format!("{} events ({summary})", self.run_bundle_event_records.len());
    }

    fn apply_eye_filter(&mut self) {
        self.store.set_eye_threshold(self.eye_min_conf);
        self.eye_status = format!(
            "Filtered {} samples ({})",
            self.store.eye_filtered().len(),
            self.eye_layout.label()
        );
    }

    fn show_hrv_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls").show(ctx, |ui| {
            ui.heading("Controls");
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);
            let slider =
                ui.add(egui::Slider::new(&mut self.fs, 50.0..=2000.0).text("Sampling freq (Hz)"));
            if slider.changed() {
                self.status = format!("Sampling frequency set to {:.1} Hz", self.fs);
            }

            let psd_slider = ui.add(
                egui::Slider::new(&mut self.psd_interp_fs, 2.0..=64.0).text("PSD interp fs (Hz)"),
            );
            if psd_slider.changed() {
                self.store.set_psd_interp_fs(self.psd_interp_fs);
                let _ = self
                    .store
                    .command_sender()
                    .send(StreamCommand::SetPsdInterpFs(self.psd_interp_fs));
                if let Some(events) = self.store.events() {
                    let fs = self.store.ecg().map(|ts| ts.fs).unwrap_or(self.fs).max(1.0);
                    self.store.submit_events(events.clone(), fs);
                }
                self.status = format!("PSD interpolation set to {:.1} Hz", self.psd_interp_fs);
            }

            if ui.button("Load raw ECG").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("ECG", &["txt", "csv", "ecg", "dat", "hea", "atr"])
                    .pick_file()
                {
                    if let Err(err) = self.load_raw(&path) {
                        self.status = err;
                    }
                }
            }

            if ui.button("Load beat annotations").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter(
                        "Annotations",
                        &["txt", "ann", "idx", "events", "atr", "tsv"],
                    )
                    .pick_file()
                {
                    if let Err(err) = self.load_annotations(&path) {
                        self.status = err;
                    }
                }
            }

            ui.separator();
            let detect_enabled = self.store.ecg().is_some();
            if ui
                .add_enabled(detect_enabled, egui::Button::new("Detect beats"))
                .clicked()
            {
                if let Err(err) = self.run_detection() {
                    self.status = err;
                }
            }

            let stream_label = if self.stream_simulator.is_some() {
                "Stop synthetic stream"
            } else {
                "Stream synthetic beats"
            };
            if ui.button(stream_label).clicked() {
                self.toggle_streaming();
            }

            if ui.button("Process synthetic ECG").clicked() {
                if let Err(err) = self.process_synthetic_ecg() {
                    self.status = err;
                }
            }

            ui.add_space(6.0);
            ui.group(|ui| {
                ui.heading("Live HRV snapshot");
                if let Some(rr) = self.store.rr_series() {
                    let hr = heart_rate_from_rr(rr).unwrap_or(0.0);
                    let mean_rr = average_rr(rr).unwrap_or(0.0);
                    let latest_rr = *rr.rr.last().unwrap_or(&0.0);
                    ui.horizontal(|ui| {
                        ui.label(format!("Heart rate: {hr:.1} bpm"));
                        ui.label(format!("Mean RR: {mean_rr:.3}s"));
                        ui.label(format!("Latest RR: {latest_rr:.3}s"));
                    });
                    ui.horizontal(|ui| {
                        if let Some(hrv) = self.store.hrv_time() {
                            ui.label(format!("RMSSD: {:.3}s", hrv.rmssd));
                        }
                        if let Some(psd) = self.store.hrv_psd() {
                            ui.label(format!("LF/HF: {:.2}", psd.lf_hf));
                        }
                        ui.label(format!("Beats: {}", rr.rr.len()));
                    });
                } else {
                    ui.label("Waiting for RR events to compute live stats...");
                }
                let can_export_hrv =
                    self.store.rr_series().is_some() || self.store.events().is_some();
                if ui
                    .add_enabled(can_export_hrv, egui::Button::new("Export HRV snapshot"))
                    .clicked()
                {
                    match self.export_hrv_snapshot() {
                        Ok(HrvExportOutcome::Exported(path)) => {
                            self.status = format!("Exported HRV snapshot to {}", path.display());
                        }
                        Ok(HrvExportOutcome::Cancelled) => {
                            self.status = "Export cancelled".into();
                        }
                        Ok(HrvExportOutcome::NoData) => {
                            self.status = "No RR data available".into();
                        }
                        Err(err) => {
                            self.status = format!("Export failed: {err}");
                        }
                    }
                }
            });

            ui.group(|ui| {
                ui.heading("Signal Quality (SQI)");
                if let Some(sqi) = self.store.sqi() {
                    let ok = sqi.is_acceptable();
                    let color = if ok {
                        egui::Color32::LIGHT_GREEN
                    } else {
                        egui::Color32::LIGHT_RED
                    };
                    ui.colored_label(
                        color,
                        format!(
                            "SQI status: {}",
                            if ok { "acceptable" } else { "needs review" }
                        ),
                    );
                    ui.horizontal(|ui| {
                        ui.label(format!("Kurtosis: {:.2}", sqi.kurtosis));
                        ui.label(format!("SNR: {:.1} dB", sqi.snr));
                        ui.label(format!("RR CV: {:.2}", sqi.rr_cv));
                    });
                    ui.add(
                        egui::ProgressBar::new((sqi.snr / 20.0).clamp(0.0, 1.0) as f32).text("SNR"),
                    );
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(((0.2 - sqi.rr_cv).max(0.0) / 0.2) as f32)
                                .text("RR CV"),
                        );
                        ui.label(format!("Spectral entropy: {:.2}", sqi.spectral_entropy));
                        ui.label(format!("PPG spikes: {:.2}", sqi.ppg_spike_ratio));
                    });
                } else {
                    ui.label("SQI requires ECG + RR before it can evaluate signal quality.");
                }
            });

            if let Some(hist) = self.store.rr_histogram() {
                ui.separator();
                ui.heading("RR histogram");
                Plot::new("rr_hist_plot").height(140.0).show(ui, |plot_ui| {
                    plot_plot_figure(plot_ui, hist);
                });
            }

            ui.separator();
            ui.heading("Live LSL stream");
            ui.horizontal(|ui| {
                ui.label("Type");
                ui.text_edit_singleline(&mut self.lsl_query);
                if ui.button("Refresh streams").clicked() {
                    self.refresh_lsl_streams();
                }
            });
            ui.add(egui::Slider::new(&mut self.lsl_chunk_samples, 32..=2048).text("Chunk samples"));
            ui.add(
                egui::DragValue::new(&mut self.lsl_fs_hint)
                    .range(1.0..=4000.0)
                    .speed(1.0)
                    .suffix(" Hz"),
            );
            let streams = self.store.lsl_streams();
            if streams.is_empty() {
                ui.label("No LSL streams discovered (refresh to search for a type).");
            } else {
                let selected = self
                    .lsl_selected_stream
                    .and_then(|idx| streams.get(idx))
                    .map(|stream| stream.name.clone())
                    .unwrap_or_else(|| "Select stream".into());
                egui::ComboBox::from_label("Stream")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (idx, stream) in streams.iter().enumerate() {
                            let label = format!(
                                "{} ({}) @ {:.1} Hz · {} ch",
                                stream.name, stream.source_id, stream.fs, stream.channels
                            );
                            if ui
                                .selectable_label(self.lsl_selected_stream == Some(idx), label)
                                .clicked()
                            {
                                self.lsl_selected_stream = Some(idx);
                                self.lsl_selected_channel = 0;
                            }
                        }
                    });
                if let Some(idx) = self.lsl_selected_stream {
                    if let Some(stream) = streams.get(idx) {
                        let max_channel = stream.channels.max(1) as usize;
                        ui.add(
                            egui::Slider::new(
                                &mut self.lsl_selected_channel,
                                0..=max_channel.saturating_sub(1),
                            )
                            .text("Channel"),
                        );
                        ui.label(format!(
                            "{} channels @ {:.1} Hz ({:?})",
                            stream.channels, stream.fs, stream.format
                        ));
                    }
                }
            }
            let lsl_label = if self.store.is_lsl_streaming() {
                "Stop LSL inlet"
            } else {
                "Start LSL inlet"
            };
            if ui.button(lsl_label).clicked() {
                self.toggle_lsl_stream();
            }
            match self.store.lsl_status() {
                LslStatus::Idle => {
                    ui.label("LSL: idle");
                }
                LslStatus::Resolving { query } => {
                    ui.label(format!("Resolving '{query}' ..."));
                }
                LslStatus::Connected {
                    name,
                    source_id,
                    channels,
                    fs,
                    query,
                    channel,
                } => {
                    ui.label(format!(
                        "Connected to {name} ({source_id}) via '{query}' channel {channel}"
                    ));
                    ui.label(format!("{channels} channels @ {:.1} Hz", fs));
                }
                LslStatus::Error(msg) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("LSL error: {msg}"));
                }
            }

            ui.separator();
            ui.heading("Parquet recording");
            let recording_status = self.store.recording_status().clone();
            let recording_active = matches!(
                recording_status,
                RecordingStatus::Active { .. } | RecordingStatus::Starting { .. }
            );
            if ui
                .add_enabled(
                    !recording_active,
                    egui::Button::new("Start Parquet recording"),
                )
                .clicked()
            {
                self.start_recording_dialog();
            }
            if ui
                .add_enabled(recording_active, egui::Button::new("Stop recording"))
                .clicked()
            {
                self.stop_recording();
            }
            match recording_status {
                RecordingStatus::Idle => ui.label("Recorder idle"),
                RecordingStatus::Starting { path } => {
                    ui.label(format!("Starting recorder at {}", path.display()))
                }
                RecordingStatus::Active { path, samples } => ui.label(format!(
                    "Recording {} samples → {}",
                    samples,
                    path.display()
                )),
                RecordingStatus::Error(msg) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("Recorder error: {msg}"))
                }
            };

            ui.separator();
            if let Some(raw) = &self.raw_path {
                ui.horizontal(|ui| {
                    ui.label("Raw: ");
                    ui.monospace(raw);
                });
            }
            if let Some(ann) = &self.annotation_path {
                ui.horizontal(|ui| {
                    ui.label("Annotations: ");
                    ui.monospace(ann);
                });
            }

            ui.separator();
            ui.heading("Run bundle");
            ui.label("Run bundle TSV column names (leave blank to ignore optional fields).");
            ui.horizontal(|ui| {
                ui.label("Onset column");
                ui.text_edit_singleline(&mut self.run_bundle_onset_column);
            });
            ui.horizontal(|ui| {
                ui.label("Event type column");
                ui.text_edit_singleline(&mut self.run_bundle_event_type_column);
            });
            ui.horizontal(|ui| {
                ui.label("Duration column");
                ui.text_edit_singleline(&mut self.run_bundle_duration_column);
            });
            ui.horizontal(|ui| {
                ui.label("Label column");
                ui.text_edit_singleline(&mut self.run_bundle_label_column);
            });
            ui.horizontal(|ui| {
                ui.label("Event types");
                ui.text_edit_singleline(&mut self.run_bundle_event_filter);
            });
            ui.label("Comma-separated list of event_type values (empty = any type).");
            if ui.button("Load run bundle").clicked() {
                if let Some(path) = FileDialog::new().pick_folder() {
                    if let Err(err) = self.try_load_run_bundle(&path) {
                        self.status = err;
                    }
                }
            }
            if let Some(bundle) = &self.run_bundle_path {
                ui.label(format!("Bundle: {}", bundle));
            }
            if let Some(manifest) = &self.run_manifest {
                ui.label(format!("Task: {}", manifest.task));
                ui.label(format!(
                    "Trials: {} events: {}",
                    manifest.total_trials, manifest.total_events
                ));
                ui.label(format!("ISI: {} ms", manifest.isi_ms));
                if let Some(jitter) = manifest.isi_jitter_ms {
                    ui.label(format!("Jitter: ±{:.1} ms", jitter));
                }
                if let Some(policy) = &manifest.randomization_policy {
                    ui.label(format!("Randomization: {}", policy));
                }
            }
            if !self.run_bundle_event_summary.is_empty() {
                ui.label(format!("Matched events: {}", self.run_bundle_event_summary));
            }

            ui.separator();
            ui.label(format!("Status: {}", self.status));
            ui.label(format!("Samples: {}", self.store.ecg_len()));
            ui.label(format!("Beats: {}", self.store.events_len()));
            if let Some(rr) = self.store.rr_series() {
                let mean = rr.rr.iter().copied().sum::<f64>() / rr.rr.len() as f64;
                ui.label(format!("RR mean: {:+.3}s", mean));
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 6.0);
            ScrollArea::vertical().show(ui, |ui| {
                if self.store.ecg().is_none() {
                    ui.centered_and_justified(|ui| {
                        ui.label("Load an ECG recording to see the waveform.");
                    });
                    return;
                }

                if let Some(fig) = self.store.ecg_figure() {
                    Plot::new("ecg_plot").height(360.0).show(ui, |plot_ui| {
                        plot_plot_figure(plot_ui, fig);
                        for time in self.store.event_seconds() {
                            plot_ui.vline(
                                VLine::new(time).stroke(egui::Stroke::new(1.0, egui::Color32::RED)),
                            );
                        }
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Preparing ECG waveform...");
                    });
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.group(|ui| {
                        ui.label("HRV (time domain)");
                        if let Some(hrv) = self.store.hrv_time() {
                            ui.label(format!("AVNN: {:.3}s", hrv.avnn));
                            ui.label(format!("SDNN: {:.3}s", hrv.sdnn));
                            ui.label(format!("RMSSD: {:.3}s", hrv.rmssd));
                            ui.label(format!("pNN50: {:.2}%", hrv.pnn50 * 100.0));
                        } else {
                            ui.label("No HRV metrics available");
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label("RR intervals (first five)");
                        if let Some(rr) = self.store.rr_series() {
                            for value in rr.rr.iter().take(5) {
                                ui.label(format!("{:.3}s", value));
                            }
                            if rr.rr.len() > 5 {
                                ui.label(format!("... +{} more", rr.rr.len() - 5));
                            }
                        } else {
                            ui.label("No RR intervals yet");
                        }
                    });
                });

                if let Some(psd) = self.store.hrv_psd() {
                    ui.separator();
                    ui.label("Frequency domain");
                    ui.label(format!("LF: {:.3}", psd.lf));
                    ui.label(format!("HF: {:.3}", psd.hf));
                    ui.label(format!("VLF: {:.3}", psd.vlf));
                    ui.label(format!("LF/HF: {:.3}", psd.lf_hf));
                    if let Some(psd_fig) = self.store.psd_figure() {
                        Plot::new("psd_plot").height(180.0).show(ui, |plot_ui| {
                            plot_plot_figure(plot_ui, psd_fig);
                        });
                    }
                }

                if let Some(nl) = self.store.hrv_nonlinear() {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(format!("SD1: {:.3}s", nl.sd1));
                        ui.label(format!("SD2: {:.3}s", nl.sd2));
                        ui.label(format!("SampEn: {:.3}", nl.samp_entropy));
                        ui.label(format!("DFA alpha1: {:.3}", nl.dfa_alpha1));
                    });
                }
            });
        });
    }

    fn show_eeg_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("eeg_controls").show(ctx, |ui| {
            ui.heading("EEG Controls");
            ui.add(egui::Slider::new(&mut self.eeg_channel, 0..=15).text("Channel"));

            if ui.button("Load EDF trace").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter(
                        "EDF / WFDB",
                        &["edf", "bdf", "dat", "hea", "atr", "tsv", "csv", "txt"],
                    )
                    .pick_file()
                {
                    if let Err(err) = self.load_eeg_trace(&path, self.eeg_channel) {
                        self.eeg_status = err;
                    }
                }
            }

            if ui.button("Load BIDS events").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("TSV", &["tsv", "txt"])
                    .pick_file()
                {
                    if let Err(err) = self.load_eeg_events(&path) {
                        self.eeg_status = err;
                    }
                }
            }

            ui.separator();
            if let Some(raw) = &self.eeg_path {
                ui.horizontal(|ui| {
                    ui.label("EDF: ");
                    ui.monospace(raw);
                });
            }
            if let Some(ev) = &self.eeg_event_source {
                ui.horizontal(|ui| {
                    ui.label("Events: ");
                    ui.monospace(ev);
                });
            }

            ui.separator();
            ui.label(format!("Status: {}", self.eeg_status));
            ui.label(format!("Samples: {}", self.store.eeg_sample_count()));
            ui.label(format!("Events: {}", self.store.eeg_events().len()));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.store.eeg_sample_count() == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label("Load an EDF trace to visualize EEG.");
                });
                return;
            }

            if let Some(fig) = self.store.eeg_figure() {
                Plot::new("eeg_plot").height(320.0).show(ui, |plot_ui| {
                    plot_plot_figure(plot_ui, fig);
                    for &onset in self.store.eeg_events() {
                        plot_ui.vline(
                            VLine::new(onset)
                                .stroke(egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE)),
                        );
                    }
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Rendering EEG trace...");
                });
            }

            ui.separator();
            ui.heading("Event onsets (seconds)");
            for onset in self.store.eeg_events().iter().take(5) {
                ui.label(format!("{:.3}s", onset));
            }
            if self.store.eeg_events().len() > 5 {
                ui.label(format!("... +{} more", self.store.eeg_events().len() - 5));
            }
        });
    }

    fn show_eye_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("eye_controls").show(ctx, |ui| {
            ui.heading("Eye Tracking");
            let slider =
                ui.add(egui::Slider::new(&mut self.eye_min_conf, 0.0..=1.0).text("Min confidence"));
            if slider.changed() {
                self.apply_eye_filter();
            }
            ui.horizontal(|ui| {
                ui.label("Format");
                for layout in [EyeLayout::PupilLabs, EyeLayout::Tobii] {
                    if ui
                        .selectable_label(self.eye_layout == layout, layout.label())
                        .clicked()
                    {
                        self.eye_layout = layout;
                    }
                }
            });
            if ui.button("Reload filtering").clicked() {
                self.apply_eye_filter();
            }

            ui.separator();
            if ui.button("Load eye CSV").clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("CSV/TSV", &["csv", "tsv", "txt", "json"])
                    .pick_file()
                {
                    if let Err(err) = self.load_eye_csv(&path) {
                        self.eye_status = err;
                    }
                }
            }

            ui.separator();
            if let Some(path) = &self.eye_path {
                ui.horizontal(|ui| {
                    ui.label("File: ");
                    ui.monospace(path);
                });
            }
            ui.label(format!("Samples: {}", self.store.eye_filtered().len()));
            ui.label(format!("Status: {}", self.eye_status));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.store.eye_filtered().is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Load an eye-tracking CSV to view pupil time courses.");
                });
                return;
            }

            if let Some(fig) = self.store.eye_figure() {
                Plot::new("eye_plot").height(300.0).show(ui, |plot_ui| {
                    plot_plot_figure(plot_ui, fig);
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Preparing pupil figure...");
                });
            }

            ui.separator();
            let values: Vec<f64> = self
                .store
                .eye_filtered()
                .iter()
                .filter_map(|sample| sample.pupil_mm.map(|p| p as f64))
                .collect();
            if values.is_empty() {
                ui.label("Mean pupil: n/a");
            } else {
                let sum: f64 = values.iter().copied().sum();
                ui.label(format!("Mean pupil: {:.3} mm", sum / values.len() as f64));
            }
        });
    }

    fn show_settings_tab(&mut self, ctx: &egui::Context) {
        if !self.keys_loaded {
            self.refresh_keys();
            self.keys_loaded = true;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Security & Key Management");
            ui.horizontal(|ui| {
                ui.label("Status:");
                ui.label(&self.key_status);
                if ui.button("Refresh").clicked() {
                    self.refresh_keys();
                    self.keys_loaded = true;
                }
            });
            ui.separator();
            ui.group(|ui| {
                ui.heading("Generate a key/cert bundle");
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut self.key_name);
                    ui.label("Validity days");
                    ui.add(
                        egui::DragValue::new(&mut self.key_validity_days)
                            .range(1..=3650)
                            .suffix("d"),
                    );
                });
                if ui.button("Generate bundle").clicked() {
                    self.generate_key_action();
                }
            });
            ui.separator();
            ui.group(|ui| {
                ui.heading("Import existing bundle");
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut self.import_name);
                });
                ui.horizontal(|ui| {
                    if ui.button("Choose certificate").clicked() {
                        if let Some(path) =
                            FileDialog::new().add_filter("PEM", &["pem"]).pick_file()
                        {
                            self.import_cert = Some(path);
                        }
                    }
                    ui.label(
                        self.import_cert
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "No certificate selected".into()),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("Choose private key").clicked() {
                        if let Some(path) =
                            FileDialog::new().add_filter("PEM", &["pem"]).pick_file()
                        {
                            self.import_key = Some(path);
                        }
                    }
                    ui.label(
                        self.import_key
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "No key selected".into()),
                    );
                });
                if ui.button("Import bundle").clicked() {
                    self.import_key_action();
                }
            });
            ui.separator();
            ui.group(|ui| {
                ui.heading("Stored bundles");
                ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    egui::Grid::new("key_grid")
                        .striped(true)
                        .min_col_width(100.0)
                        .show(ui, |ui| {
                            ui.label("Name");
                            ui.label("Created");
                            ui.label("Actions");
                            ui.end_row();
                            for entry in self.key_list.clone() {
                                ui.label(&entry.name);
                                ui.label(entry.created.as_deref().unwrap_or("unknown"));
                                if ui.button("Export").clicked() {
                                    self.export_key_action(&entry.name);
                                }
                                ui.end_row();
                                ui.label(
                                    egui::RichText::new(entry.cert_path.display().to_string())
                                        .small(),
                                );
                                ui.end_row();
                            }
                        });
                });
            });
        });
    }

    fn show_landing_tab(&mut self, ctx: &egui::Context) {
        self.poll_batch_results();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Batch CLI runner");
            ui.label("Drop or select inputs, configure CLI flags, and save outputs.");
            ui.separator();

            egui::ComboBox::from_label("Command")
                .selected_text(self.landing_batch.command.title())
                .show_ui(ui, |ui| {
                    for &option in
                        [BatchCommand::BeatHrvPipeline, BatchCommand::EcgFindRpeaks].iter()
                    {
                        if ui
                            .selectable_label(self.landing_batch.command == option, option.title())
                            .clicked()
                        {
                            self.landing_batch.command = option;
                        }
                    }
                });

            ui.horizontal(|ui| {
                ui.label("Sampling rate (Hz)");
                ui.text_edit_singleline(&mut self.landing_batch.fs);
                ui.label("Min RR (s)");
                ui.text_edit_singleline(&mut self.landing_batch.min_rr);
            });
            ui.horizontal(|ui| {
                ui.label("Threshold scale");
                ui.add(egui::Slider::new(
                    &mut self.landing_batch.threshold_scale,
                    0.1..=2.0,
                ));
            });

            ui.horizontal(|ui| {
                if ui.button("Add input files").clicked() {
                    if let Some(files) = FileDialog::new()
                        .add_filter("ECG + CSV", &["txt", "csv", "dat", "hea", "edf", "json"])
                        .pick_files()
                    {
                        self.landing_batch.inputs.extend(files);
                    }
                }
                if ui.button("Clear inputs").clicked() {
                    self.landing_batch.inputs.clear();
                }
            });
            if !self.landing_batch.inputs.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        for path in &self.landing_batch.inputs {
                            ui.label(path.display().to_string());
                        }
                    });
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Select destination folder").clicked() {
                    if let Some(dest) = FileDialog::new().pick_folder() {
                        self.landing_batch.dest = Some(dest);
                    }
                }
                if let Some(dest) = &self.landing_batch.dest {
                    ui.label(dest.display().to_string());
                } else {
                    ui.label("No destination set");
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.landing_batch.use_annotations,
                    "Include annotations",
                );
                if ui.button("Select annotations").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("ATR", &["atr", "json", "txt"])
                        .pick_file()
                    {
                        self.landing_batch.annotations = Some(path);
                    }
                }
                if let Some(path) = &self.landing_batch.annotations {
                    ui.label(path.display().to_string());
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.landing_batch.use_bids, "Use BIDS events");
                if ui.button("Select BIDS TSV").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("TSV", &["tsv", "txt"])
                        .pick_file()
                    {
                        self.landing_batch.bids_events = Some(path);
                    }
                }
                if let Some(path) = &self.landing_batch.bids_events {
                    ui.label(path.display().to_string());
                }
            });

            ui.separator();
            let can_run = !self.landing_batch.inputs.is_empty()
                && self.landing_batch.dest.is_some()
                && !self.batch_running;
            if ui
                .add_enabled(can_run, egui::Button::new("Run batch CLI"))
                .clicked()
            {
                self.start_batch_run();
            }
            if self.batch_running {
                ui.label("Batch running...");
            }
            if ui.button("Clear status").clicked() {
                self.batch_summary.clear();
            }
            if !self.batch_summary.is_empty() {
                ui.separator();
                ui.label("Batch results:");
                for entry in &self.batch_summary {
                    let status = if entry.success { "Success" } else { "Error" };
                    ui.label(format!(
                        "{} – {}: {}",
                        status,
                        entry.file.display(),
                        entry.message
                    ));
                }
            }
        });
    }

    fn refresh_keys(&mut self) {
        match elf_keys::list_keys() {
            Ok(keys) => {
                self.key_list = keys;
                self.key_status = format!("Loaded {} key bundle(s)", self.key_list.len());
            }
            Err(err) => {
                self.key_list.clear();
                self.key_status = format!("Key refresh failed: {}", err);
            }
        }
    }

    fn mark_keys_dirty(&mut self) {
        self.keys_loaded = false;
    }

    fn generate_key_action(&mut self) {
        let name = self.key_name.trim();
        if name.is_empty() {
            self.key_status = "Provide a name for the key bundle".into();
            return;
        }
        let days = self.key_validity_days.min(u32::from(u16::MAX));
        match elf_keys::generate_key(name, days as u16) {
            Ok(entry) => {
                self.key_status = format!("Generated key {}", entry.name);
                self.mark_keys_dirty();
            }
            Err(err) => {
                self.key_status = format!("Generate failed: {}", err);
            }
        }
    }

    fn import_key_action(&mut self) {
        let name = self.import_name.trim();
        if name.is_empty() {
            self.key_status = "Provide a name before importing".into();
            return;
        }
        let cert = match &self.import_cert {
            Some(path) => path,
            None => {
                self.key_status = "Select a certificate to import".into();
                return;
            }
        };
        let key = match &self.import_key {
            Some(path) => path,
            None => {
                self.key_status = "Select a private key to import".into();
                return;
            }
        };
        match elf_keys::import_key(name, cert, key) {
            Ok(entry) => {
                self.key_status = format!("Imported key {}", entry.name);
                self.mark_keys_dirty();
                self.clear_import_selection();
            }
            Err(err) => {
                self.key_status = format!("Import failed: {}", err);
            }
        }
    }

    fn export_key_action(&mut self, name: &str) {
        if let Some(dest) = FileDialog::new().pick_folder() {
            match elf_keys::export_key(name, &dest) {
                Ok((cert, key)) => {
                    self.key_status = format!(
                        "Exported {} -> cert={} key={}",
                        name,
                        cert.display(),
                        key.display()
                    );
                }
                Err(err) => {
                    self.key_status = format!("Export failed: {}", err);
                }
            }
        } else {
            self.key_status = "Export cancelled".into();
        }
    }

    fn clear_import_selection(&mut self) {
        self.import_cert = None;
        self.import_key = None;
        self.import_name.clear();
    }

    fn start_batch_run(&mut self) {
        if self.batch_running || self.landing_batch.inputs.is_empty() {
            return;
        }
        let dest = match &self.landing_batch.dest {
            Some(dest) => dest.clone(),
            None => return,
        };
        let config = self.landing_batch.clone();
        let (tx, rx) = mpsc::channel();
        self.batch_receiver = Some(rx);
        self.batch_summary.clear();
        self.batch_running = true;
        thread::spawn(move || run_batch_task(config, dest, tx));
    }

    fn poll_batch_results(&mut self) {
        if let Some(rx) = &self.batch_receiver {
            loop {
                match rx.try_recv() {
                    Ok(feedback) => self.batch_summary.push(feedback),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.batch_receiver = None;
                        self.batch_running = false;
                        break;
                    }
                }
            }
        }
    }
}

fn run_batch_task(config: LandingBatch, dest: PathBuf, sender: mpsc::Sender<BatchFeedback>) {
    if let Err(err) = std::fs::create_dir_all(&dest) {
        let _ = sender.send(BatchFeedback {
            file: dest.clone(),
            success: false,
            message: format!("unable to create destination: {}", err),
        });
        return;
    }
    let inputs = config.inputs.clone();
    for input in inputs {
        let feedback = run_single_file(&config, input, &dest);
        let _ = sender.send(feedback);
    }
}

fn run_single_file(config: &LandingBatch, input: PathBuf, dest: &Path) -> BatchFeedback {
    let mut cmd = Command::new("elf");
    cmd.arg(config.command.label());
    cmd.arg("--fs").arg(&config.fs);
    cmd.arg("--min-rr-s").arg(&config.min_rr);
    if config.command == BatchCommand::BeatHrvPipeline {
        cmd.arg("--threshold-scale")
            .arg(config.threshold_scale.to_string());
    }
    if config.use_annotations {
        if let Some(path) = &config.annotations {
            cmd.arg("--annotations").arg(path);
        }
    }
    if config.use_bids && config.command == BatchCommand::BeatHrvPipeline {
        if let Some(path) = &config.bids_events {
            cmd.arg("--bids-events").arg(path);
        }
    }
    cmd.arg("--input").arg(&input);

    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                let out_path = batch_output_path(&input, dest);
                match std::fs::write(&out_path, &output.stdout) {
                    Ok(()) => BatchFeedback {
                        file: input,
                        success: true,
                        message: format!("written {} bytes", output.stdout.len()),
                    },
                    Err(err) => BatchFeedback {
                        file: input,
                        success: false,
                        message: format!("failed to write output: {}", err),
                    },
                }
            } else {
                BatchFeedback {
                    file: input,
                    success: false,
                    message: format!(
                        "command failed (code {}) {}",
                        output.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&output.stderr)
                    ),
                }
            }
        }
        Err(err) => BatchFeedback {
            file: input,
            success: false,
            message: format!("failed to spawn elf: {}", err),
        },
    }
}

fn batch_output_path(input: &Path, dest: &Path) -> PathBuf {
    let file_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let (base, extension) = match file_name.rfind('.') {
        Some(idx) => (&file_name[..idx], &file_name[idx..]),
        None => (file_name, ""),
    };
    let output_name = format!("{}_elfout{}", base, extension);
    dest.join(output_name)
}

impl Drop for ElfApp {
    fn drop(&mut self) {
        if let Some(sim) = self.stream_simulator.take() {
            sim.stop();
        }
    }
}

impl eframe::App for ElfApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(14.0, 8.0);
            style.spacing.button_padding = egui::vec2(14.0, 7.0);
            style.spacing.window_margin = Margin::same(12.0);
            let visuals = &mut style.visuals;
            visuals.widgets.inactive.rounding = 8.0.into();
            visuals.widgets.hovered.rounding = 8.0.into();
            visuals.selection.bg_fill = Color32::from_rgb(90, 160, 255);
            visuals.extreme_bg_color = Color32::from_rgb(18, 18, 24);
            visuals.window_fill = Color32::from_rgb(24, 24, 32);
        });
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.heading("Extensible Lab Framework — Multimodal Viewer");
                ui.label("Choose a tab to explore ECG/HRV, EEG, or eye-tracking workflows.");
                ui.horizontal(|ui| {
                    for tab in GuiTab::all() {
                        let selected = self.active_tab == tab;
                        if ui.selectable_label(selected, tab.title()).clicked() {
                            self.active_tab = tab;
                        }
                    }
                });
            });
        });

        self.store.prepare_active_tab(self.active_tab);

        match self.active_tab {
            GuiTab::Landing => self.show_landing_tab(ctx),
            GuiTab::Hrv => self.show_hrv_tab(ctx),
            GuiTab::Eeg => self.show_eeg_tab(ctx),
            GuiTab::Eye => self.show_eye_tab(ctx),
            GuiTab::Settings => self.show_settings_tab(ctx),
        }

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| match self.active_tab {
                GuiTab::Landing => ui.label("Landing content is under construction."),
                GuiTab::Hrv => ui.label("Ready to inspect ECGs and beat annotations."),
                GuiTab::Eeg => ui.label("Ready to explore EEG traces and events."),
                GuiTab::Eye => ui.label("Ready to explore eye-tracking data."),
                GuiTab::Settings => ui.label("Manage TLS keys/certs for secure transports."),
            });
        });
    }
}

fn plot_plot_figure(plot_ui: &mut egui_plot::PlotUi, figure: &Figure) {
    for series in &figure.series {
        match series {
            Series::Line(line) => {
                plot_ui.line(
                    Line::new(line.points.clone())
                        .stroke(stroke_from_style(&line.style))
                        .name(line.name.clone()),
                );
            }
        }
    }
}

fn stroke_from_style(style: &Style) -> egui::Stroke {
    egui::Stroke::new(style.width, color_from_u32(style.color.0))
}

fn color_from_u32(color: u32) -> egui::Color32 {
    let r = ((color >> 16) & 0xFF) as u8;
    let g = ((color >> 8) & 0xFF) as u8;
    let b = (color & 0xFF) as u8;
    egui::Color32::from_rgb(r, g, b)
}
