use crossbeam_channel::{bounded, Sender};
use eframe::{egui, egui::ViewportBuilder};
use egui_plot::{Line, Plot, VLine};
use elf_lib::detectors::ecg::{run_beat_hrv_pipeline, EcgPipelineConfig};
use elf_lib::io::{eeg as eeg_io, eye as eye_io, text as text_io, wfdb as wfdb_io};
use elf_lib::plot::{Figure, Series, Style};
use elf_lib::signal::{Events, TimeSeries};
use rfd::FileDialog;
use std::env;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
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
}

impl GuiTab {
    fn title(&self) -> &'static str {
        match self {
            GuiTab::Landing => "Landing",
            GuiTab::Hrv => "ECG / HRV",
            GuiTab::Eeg => "EEG",
            GuiTab::Eye => "Eye",
        }
    }

    fn all() -> [GuiTab; 4] {
        [GuiTab::Landing, GuiTab::Hrv, GuiTab::Eeg, GuiTab::Eye]
    }
}

mod router;
mod run_loader;
mod store;

use router::{LslStatus, RecordingStatus, StreamCommand, StreamingStateRouter};
use run_loader::{events_from_times, load_events, load_manifest, RunManifest};
use store::Store;

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
    status: String,
    active_tab: GuiTab,
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
    lsl_query: String,
    lsl_chunk_samples: usize,
    lsl_fs_hint: f64,
}

impl Default for ElfApp {
    fn default() -> Self {
        Self {
            store: StreamingStateRouter::new(Store::new()),
            raw_path: None,
            annotation_path: None,
            fs: 250.0,
            status: "No recording loaded".into(),
            active_tab: GuiTab::Landing,
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
            lsl_query: "ECG".into(),
            lsl_chunk_samples: 256,
            lsl_fs_hint: 250.0,
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

    fn toggle_lsl_stream(&mut self) {
        if self.store.is_lsl_streaming() {
            self.store.stop_lsl_stream();
            self.status = "Stopped LSL inlet".into();
            return;
        }
        let query = self.lsl_query.trim();
        if query.is_empty() {
            self.status = "Provide an LSL stream type before connecting".into();
            return;
        }
        let chunk = self.lsl_chunk_samples.max(1);
        let fs_hint = if self.lsl_fs_hint.is_finite() && self.lsl_fs_hint > 0.0 {
            Some(self.lsl_fs_hint)
        } else {
            None
        };
        match self
            .store
            .start_lsl_stream(query.to_string(), chunk, fs_hint)
        {
            Ok(_) => {
                self.status = format!("Resolving LSL stream ({query})");
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
        let times =
            load_events(&events_path).map_err(|e| format!("Run events load failed: {}", e))?;
        let fs = self.store.ecg().map(|ts| ts.fs).unwrap_or(self.fs).max(1.0);
        let events = events_from_times(&times, fs);
        self.store.submit_events(events, fs);
        let manifest = load_manifest(&manifest_path)
            .map_err(|e| format!("Run manifest load failed: {}", e))?;
        self.run_bundle_path = Some(path.display().to_string());
        self.run_manifest = Some(manifest);
        self.status = format!(
            "Loaded run bundle from {} ({} stimuli)",
            path.display(),
            times.len()
        );
        Ok(())
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
            let slider =
                ui.add(egui::Slider::new(&mut self.fs, 50.0..=2000.0).text("Sampling freq (Hz)"));
            if slider.changed() {
                self.status = format!("Sampling frequency set to {:.1} Hz", self.fs);
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

            ui.separator();
            ui.heading("Live LSL stream");
            ui.horizontal(|ui| {
                ui.label("Type");
                ui.text_edit_singleline(&mut self.lsl_query);
            });
            ui.add(egui::Slider::new(&mut self.lsl_chunk_samples, 32..=2048).text("Chunk samples"));
            ui.add(
                egui::DragValue::new(&mut self.lsl_fs_hint)
                    .range(1.0..=4000.0)
                    .speed(1.0)
                    .suffix(" Hz"),
            );
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
                } => {
                    ui.label(format!("Connected to {name} ({source_id}) via '{query}'"));
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
            GuiTab::Landing => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.heading("Landing page");
                        ui.label("More dashboards and insights coming soon.");
                    });
                });
            }
            GuiTab::Hrv => self.show_hrv_tab(ctx),
            GuiTab::Eeg => self.show_eeg_tab(ctx),
            GuiTab::Eye => self.show_eye_tab(ctx),
        }

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| match self.active_tab {
                GuiTab::Landing => ui.label("Landing content is under construction."),
                GuiTab::Hrv => ui.label("Ready to inspect ECGs and beat annotations."),
                GuiTab::Eeg => ui.label("Ready to explore EEG traces and events."),
                GuiTab::Eye => ui.label("Ready to explore eye-tracking data."),
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
