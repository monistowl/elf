use crate::{store::Store, GuiTab};
use anyhow::{anyhow, Context, Result};
use arrow::array::{Array, PrimitiveArray};
use arrow::chunk::Chunk;
use arrow::datatypes::{DataType, Field, Schema};
use arrow::error::Result as ArrowResult;
use arrow::io::parquet::write::{
    CompressionOptions as ArrowCompressionOptions, Encoding, FileWriter as ArrowFileWriter,
    RowGroupIterator as ArrowRowGroupIterator, Version, WriteOptions as ArrowWriteOptions,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use elf_lib::detectors::ecg::{run_beat_hrv_pipeline, EcgPipelineConfig};
use elf_lib::metrics::hrv::{hrv_nonlinear, hrv_psd, hrv_time, HRVNonlinear, HRVPsd, HRVTime};
use elf_lib::signal::{Events, RRSeries, TimeSeries};
use lsl::{self, ChannelFormat, ProcessingOption, Pullable};
use std::fs::File;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone)]
pub struct LslStreamInfo {
    pub query: String,
    pub name: String,
    pub source_id: String,
    pub channels: i32,
    pub fs: f64,
    pub format: ChannelFormat,
}

pub enum StreamCommand {
    ProcessEcg(TimeSeries),
    IngestEvents(Events, f64),
    DiscoverLslStreams { query: String },
    StartRecording { path: PathBuf, fs: f64 },
    StopRecording,
    Shutdown,
}

enum StreamUpdate {
    Ecg(TimeSeries),
    Events(Events),
    Hrv {
        rr: RRSeries,
        hrv_time: HRVTime,
        hrv_psd: HRVPsd,
        hrv_nonlinear: HRVNonlinear,
    },
    Recording(RecordingStatus),
    Lsl(LslStatus),
    LslStreams(Vec<LslStreamInfo>),
}

#[derive(Debug, Clone)]
pub enum RecordingStatus {
    Idle,
    Starting { path: PathBuf },
    Active { path: PathBuf, samples: usize },
    Error(String),
}

impl Default for RecordingStatus {
    fn default() -> Self {
        RecordingStatus::Idle
    }
}

#[derive(Debug, Clone)]
pub enum LslStatus {
    Idle,
    Resolving {
        query: String,
    },
    Connected {
        query: String,
        name: String,
        source_id: String,
        channels: i32,
        channel: usize,
        fs: f64,
    },
    Error(String),
}

impl Default for LslStatus {
    fn default() -> Self {
        LslStatus::Idle
    }
}

struct LslStreamHandle {
    stop_tx: Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl LslStreamHandle {
    fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct StreamingStateRouter {
    store: Store,
    active_tab: GuiTab,
    command_tx: Sender<StreamCommand>,
    update_tx: Sender<StreamUpdate>,
    update_rx: Receiver<StreamUpdate>,
    worker: Option<JoinHandle<()>>,
    recording_status: RecordingStatus,
    lsl_status: LslStatus,
    lsl_stream: Option<LslStreamHandle>,
    lsl_streams: Vec<LslStreamInfo>,
}

impl StreamingStateRouter {
    pub fn new(store: Store) -> Self {
        let (command_tx, command_rx) = bounded(32);
        let (update_tx, update_rx) = bounded(32);
        let worker_tx = update_tx.clone();
        let worker = std::thread::spawn(move || RouterWorker::new(command_rx, worker_tx).run());
        Self {
            store,
            active_tab: GuiTab::Landing,
            command_tx,
            update_tx,
            update_rx,
            worker: Some(worker),
            recording_status: RecordingStatus::default(),
            lsl_status: LslStatus::default(),
            lsl_stream: None,
            lsl_streams: Vec::new(),
        }
    }

    pub fn prepare_active_tab(&mut self, tab: GuiTab) {
        self.active_tab = tab;
        self.route_pending_updates();
        self.store.prepare(tab);
    }

    pub fn command_sender(&self) -> Sender<StreamCommand> {
        self.command_tx.clone()
    }

    #[allow(dead_code)]
    pub fn submit_ecg(&self, ts: TimeSeries) {
        let _ = self.command_tx.send(StreamCommand::ProcessEcg(ts));
    }

    #[allow(dead_code)]
    pub fn submit_events(&self, events: Events, fs: f64) {
        let _ = self
            .command_tx
            .send(StreamCommand::IngestEvents(events, fs));
    }

    pub fn lsl_streams(&self) -> &[LslStreamInfo] {
        &self.lsl_streams
    }

    pub fn discover_lsl_streams(&self, query: String) -> Result<()> {
        self.command_tx
            .send(StreamCommand::DiscoverLslStreams { query })
            .map_err(|e| anyhow!("Failed to request stream discovery: {e}"))
    }

    pub fn start_recording<P: Into<PathBuf>>(&self, path: P, fs: f64) -> Result<()> {
        if fs <= 0.0 {
            return Err(anyhow!("Sampling frequency must be positive"));
        }
        self.command_tx
            .send(StreamCommand::StartRecording {
                path: path.into(),
                fs,
            })
            .map_err(|e| anyhow!("Failed to start recording: {e}"))
    }

    pub fn stop_recording(&self) -> Result<()> {
        self.command_tx
            .send(StreamCommand::StopRecording)
            .map_err(|e| anyhow!("Failed to stop recording: {e}"))
    }

    pub fn recording_status(&self) -> &RecordingStatus {
        &self.recording_status
    }

    pub fn start_lsl_stream(
        &mut self,
        query: String,
        source_id: String,
        channel: usize,
        chunk_size: usize,
        fs_hint: Option<f64>,
    ) -> Result<()> {
        if self.lsl_stream.is_some() {
            return Err(anyhow!("LSL stream already running"));
        }
        let chunk_size = chunk_size.max(1);
        let (stop_tx, stop_rx) = bounded(1);
        let command_tx = self.command_sender();
        let update_tx = self.update_tx.clone();
        let handle = std::thread::spawn(move || {
            let result = run_lsl_loop(
                query,
                source_id,
                channel,
                chunk_size,
                fs_hint,
                command_tx,
                update_tx.clone(),
                stop_rx,
            );
            if let Err(err) = result {
                let _ = update_tx.send(StreamUpdate::Lsl(LslStatus::Error(err.to_string())));
            } else {
                let _ = update_tx.send(StreamUpdate::Lsl(LslStatus::Idle));
            }
        });
        self.lsl_stream = Some(LslStreamHandle {
            stop_tx,
            handle: Some(handle),
        });
        Ok(())
    }

    pub fn stop_lsl_stream(&mut self) {
        if let Some(handle) = self.lsl_stream.take() {
            handle.stop();
            let _ = self.update_tx.send(StreamUpdate::Lsl(LslStatus::Idle));
        }
    }

    pub fn lsl_status(&self) -> &LslStatus {
        &self.lsl_status
    }

    pub fn is_lsl_streaming(&self) -> bool {
        self.lsl_stream.is_some()
    }

    fn route_pending_updates(&mut self) {
        while let Ok(update) = self.update_rx.try_recv() {
            match update {
                StreamUpdate::Ecg(ts) => self.store.set_ecg(ts),
                StreamUpdate::Events(ev) => self.store.set_events(ev),
                StreamUpdate::Hrv {
                    rr,
                    hrv_time,
                    hrv_psd,
                    hrv_nonlinear,
                } => {
                    self.store
                        .apply_stream_metrics(rr, hrv_time, hrv_psd, hrv_nonlinear);
                }
                StreamUpdate::Recording(status) => {
                    self.recording_status = status;
                }
                StreamUpdate::Lsl(status) => {
                    self.lsl_status = status;
                }
                StreamUpdate::LslStreams(streams) => {
                    self.lsl_streams = streams;
                }
            }
        }
    }
}

struct RouterWorker {
    command_rx: Receiver<StreamCommand>,
    update_tx: Sender<StreamUpdate>,
    recorder: Option<ParquetRecorder>,
}

impl RouterWorker {
    fn new(command_rx: Receiver<StreamCommand>, update_tx: Sender<StreamUpdate>) -> Self {
        Self {
            command_rx,
            update_tx,
            recorder: None,
        }
    }

    fn run(mut self) {
        while let Ok(command) = self.command_rx.recv() {
            match command {
                StreamCommand::ProcessEcg(ts) => self.handle_ecg(ts),
                StreamCommand::IngestEvents(events, fs) => {
                    let _ = self.update_tx.send(StreamUpdate::Events(events.clone()));
                    publish_metrics(&events, fs, &self.update_tx);
                }
                StreamCommand::DiscoverLslStreams { query } => {
                    self.discover_lsl_streams(&query);
                }
                StreamCommand::StartRecording { path, fs } => {
                    self.start_recording(path, fs);
                }
                StreamCommand::StopRecording => {
                    self.stop_recording();
                }
                StreamCommand::Shutdown => break,
            }
        }
        self.stop_recording();
    }

    fn discover_lsl_streams(&self, query: &str) {
        let _ = self.update_tx.send(StreamUpdate::Lsl(LslStatus::Resolving {
            query: query.to_string(),
        }));
        match lsl::resolve_byprop("type", query, 32, lsl::FOREVER) {
            Ok(streams) => {
                let infos: Vec<LslStreamInfo> = streams
                    .into_iter()
                    .map(|info| LslStreamInfo {
                        query: query.to_string(),
                        name: info.stream_name(),
                        source_id: info.source_id(),
                        channels: info.channel_count(),
                        fs: info.nominal_srate().max(0.0),
                        format: info.channel_format(),
                    })
                    .collect();
                let empty = infos.is_empty();
                let _ = self.update_tx.send(StreamUpdate::LslStreams(infos));
                if empty {
                    let _ = self.update_tx.send(StreamUpdate::Lsl(LslStatus::Idle));
                }
            }
            Err(err) => {
                let _ = self
                    .update_tx
                    .send(StreamUpdate::Lsl(LslStatus::Error(format!(
                        "Stream discovery failed: {err}"
                    ))));
            }
        }
    }

    fn handle_ecg(&mut self, ts: TimeSeries) {
        let _ = self.update_tx.send(StreamUpdate::Ecg(ts.clone()));
        let cfg = EcgPipelineConfig::default();
        let result = run_beat_hrv_pipeline(&ts, &cfg);
        let _ = self
            .update_tx
            .send(StreamUpdate::Events(result.events.clone()));
        publish_metrics(&result.events, result.fs, &self.update_tx);
        self.append_recording(&ts.data, ts.fs);
    }

    fn start_recording(&mut self, path: PathBuf, fs: f64) {
        let _ = self
            .update_tx
            .send(StreamUpdate::Recording(RecordingStatus::Starting {
                path: path.clone(),
            }));
        if self.recorder.is_some() {
            let _ = self
                .update_tx
                .send(StreamUpdate::Recording(RecordingStatus::Error(
                    "Recorder already active".into(),
                )));
            return;
        }
        match ParquetRecorder::new(path.clone(), fs) {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                let _ = self
                    .update_tx
                    .send(StreamUpdate::Recording(RecordingStatus::Active {
                        path,
                        samples: 0,
                    }));
            }
            Err(err) => {
                let _ = self
                    .update_tx
                    .send(StreamUpdate::Recording(RecordingStatus::Error(format!(
                        "Recording failed: {err}"
                    ))));
            }
        }
    }

    fn append_recording(&mut self, samples: &[f64], fs: f64) {
        if samples.is_empty() {
            return;
        }
        let recorder_active = self.recorder.is_some();
        if !recorder_active {
            return;
        }
        let mut error_message = None;
        if let Some(recorder) = self.recorder.as_mut() {
            match recorder.append(samples, fs) {
                Ok(total) => {
                    let _ = self
                        .update_tx
                        .send(StreamUpdate::Recording(RecordingStatus::Active {
                            path: recorder.path().to_path_buf(),
                            samples: total,
                        }));
                }
                Err(err) => {
                    error_message = Some(err.to_string());
                }
            }
        }
        if let Some(message) = error_message {
            let recorder = self.recorder.take();
            if let Some(recorder) = recorder {
                let _ = recorder.finish();
            }
            let _ = self
                .update_tx
                .send(StreamUpdate::Recording(RecordingStatus::Error(format!(
                    "Recording error: {message}"
                ))));
        }
    }

    fn stop_recording(&mut self) {
        if let Some(recorder) = self.recorder.take() {
            if let Err(err) = recorder.finish() {
                let _ = self
                    .update_tx
                    .send(StreamUpdate::Recording(RecordingStatus::Error(format!(
                        "Recorder close failed: {err}"
                    ))));
                return;
            }
        }
        let _ = self
            .update_tx
            .send(StreamUpdate::Recording(RecordingStatus::Idle));
    }
}

fn publish_metrics(events: &Events, fs: f64, update_tx: &Sender<StreamUpdate>) {
    let rr = RRSeries::from_events(events, fs);
    let hrv_time = hrv_time(&rr);
    let hrv_psd = hrv_psd(&rr, 4.0);
    let hrv_nonlinear = hrv_nonlinear(&rr);
    let _ = update_tx.send(StreamUpdate::Hrv {
        rr,
        hrv_time,
        hrv_psd,
        hrv_nonlinear,
    });
}

fn run_lsl_loop(
    query: String,
    source_id: String,
    channel: usize,
    chunk_size: usize,
    fs_hint: Option<f64>,
    command_tx: Sender<StreamCommand>,
    update_tx: Sender<StreamUpdate>,
    stop_rx: Receiver<()>,
) -> Result<()> {
    let _ = update_tx.send(StreamUpdate::Lsl(LslStatus::Resolving {
        query: query.clone(),
    }));
    let streams = match lsl::resolve_byprop("source_id", &source_id, 1, lsl::FOREVER) {
        Ok(list) if !list.is_empty() => list,
        _ => {
            let fallback = lsl::resolve_byprop("type", &query, 1, lsl::FOREVER)
                .map_err(|err| anyhow!("Failed to resolve LSL stream: {err:?}"))?;
            if fallback.is_empty() {
                return Err(anyhow!("No LSL stream found for source {source_id}"));
            }
            fallback
        }
    };
    let info = streams
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No LSL stream available for {query}"))?;
    let fs = if info.nominal_srate() > 0.0 {
        info.nominal_srate()
    } else {
        fs_hint.unwrap_or(250.0)
    };
    if fs <= 0.0 {
        return Err(anyhow!(
            "LSL stream {} reported invalid sample rate",
            info.stream_name()
        ));
    }
    let name = info.stream_name();
    let source_id = info.source_id();
    let channels = info.channel_count();
    if channel >= channels as usize {
        return Err(anyhow!(
            "Requested channel {channel} > {} available channels",
            channels
        ));
    }
    let format = info.channel_format();
    let inlet = lsl::StreamInlet::new(&info, chunk_size as i32, 0, true)
        .map_err(|err| anyhow!("Failed to open LSL inlet: {err:?}"))?;
    inlet
        .set_postprocessing(&[
            ProcessingOption::ClockSync,
            ProcessingOption::Dejitter,
            ProcessingOption::Monotonize,
            ProcessingOption::Threadsafe,
        ])
        .map_err(|err| anyhow!("Failed to configure LSL inlet: {err:?}"))?;
    let _ = update_tx.send(StreamUpdate::Lsl(LslStatus::Connected {
        query: query.clone(),
        name,
        source_id,
        channels,
        channel,
        fs,
    }));

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }
        let samples = match format {
            ChannelFormat::Float32 => {
                let (chunk, _): (Vec<Vec<f32>>, _) = inlet
                    .pull_chunk()
                    .map_err(|err| anyhow!("LSL read failed: {err:?}"))?;
                chunk
                    .into_iter()
                    .filter_map(|sample| sample.get(channel).copied())
                    .map(|v| v as f64)
                    .collect::<Vec<f64>>()
            }
            ChannelFormat::Double64 => {
                let (chunk, _): (Vec<Vec<f64>>, _) = inlet
                    .pull_chunk()
                    .map_err(|err| anyhow!("LSL read failed: {err:?}"))?;
                chunk
                    .into_iter()
                    .filter_map(|sample| sample.get(channel).copied())
                    .collect::<Vec<f64>>()
            }
            other => {
                return Err(anyhow!("Unsupported LSL channel format: {other:?}"));
            }
        };
        if samples.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let ts = TimeSeries { fs, data: samples };
        if command_tx.send(StreamCommand::ProcessEcg(ts)).is_err() {
            break;
        }
    }
    Ok(())
}

struct ParquetRecorder {
    writer: ArrowFileWriter<File>,
    schema: Schema,
    encodings: Vec<Vec<Encoding>>,
    options: ArrowWriteOptions,
    path: PathBuf,
    next_index: i64,
    fs: f64,
}

impl ParquetRecorder {
    fn new(path: PathBuf, fs: f64) -> Result<Self> {
        if fs <= 0.0 {
            return Err(anyhow!("Recording requires a positive sampling rate"));
        }
        let schema = Schema::from(vec![
            Field::new("sample_index", DataType::Int64, false),
            Field::new("timestamp", DataType::Float64, false),
            Field::new("value", DataType::Float64, false),
        ]);
        let options = ArrowWriteOptions {
            write_statistics: false,
            version: Version::V2,
            compression: ArrowCompressionOptions::Uncompressed,
            data_pagesize_limit: None,
        };
        let encodings = vec![
            vec![Encoding::Plain],
            vec![Encoding::Plain],
            vec![Encoding::Plain],
        ];
        let file = File::create(&path)
            .with_context(|| format!("Failed to create Parquet recording at {}", path.display()))?;
        let writer = ArrowFileWriter::try_new(file, schema.clone(), options)
            .context("Failed to initialize Parquet writer")?;
        Ok(Self {
            writer,
            schema,
            encodings,
            options,
            path,
            next_index: 0,
            fs,
        })
    }

    fn append(&mut self, samples: &[f64], fs: f64) -> Result<usize> {
        if samples.is_empty() {
            return Ok(self.next_index as usize);
        }
        if (fs - self.fs).abs() > f64::EPSILON {
            return Err(anyhow!(
                "Recorder sample rate mismatch: expected {:.3} Hz, got {:.3} Hz",
                self.fs,
                fs
            ));
        }
        let indices: Vec<i64> = (0..samples.len())
            .map(|offset| self.next_index + offset as i64)
            .collect();
        self.next_index += samples.len() as i64;
        let timestamps: Vec<f64> = indices.iter().map(|idx| *idx as f64 / self.fs).collect();

        let chunk = Chunk::try_new(vec![
            Arc::new(PrimitiveArray::<i64>::from_vec(indices)) as Arc<dyn Array>,
            Arc::new(PrimitiveArray::<f64>::from_vec(timestamps)),
            Arc::new(PrimitiveArray::<f64>::from_vec(samples.to_vec())),
        ])?;

        let mut row_groups = ArrowRowGroupIterator::try_new(
            std::iter::once(ArrowResult::Ok(chunk)),
            &self.schema,
            self.options,
            self.encodings.clone(),
        )?;
        if let Some(group) = row_groups.next() {
            let row_group = group?;
            self.writer
                .write(row_group)
                .context("Failed to write Parquet row group")?;
        }
        Ok(self.next_index as usize)
    }

    fn finish(mut self) -> Result<()> {
        self.writer
            .end(None)
            .context("Failed to finalize Parquet file")?;
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Deref for StreamingStateRouter {
    type Target = Store;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl DerefMut for StreamingStateRouter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store
    }
}

impl Drop for StreamingStateRouter {
    fn drop(&mut self) {
        self.stop_lsl_stream();
        let _ = self.command_tx.send(StreamCommand::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}
