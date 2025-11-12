use crate::{store::Store, GuiTab};
use crossbeam_channel::{bounded, Receiver, Sender};
use elf_lib::detectors::ecg::{run_beat_hrv_pipeline, EcgPipelineConfig};
use elf_lib::metrics::hrv::{hrv_nonlinear, hrv_psd, hrv_time, HRVNonlinear, HRVPsd, HRVTime};
use elf_lib::signal::{Events, RRSeries, TimeSeries};
use std::ops::{Deref, DerefMut};
use std::thread::JoinHandle;

pub enum StreamCommand {
    ProcessEcg(TimeSeries),
    IngestEvents(Events, f64),
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
}

pub struct StreamingStateRouter {
    store: Store,
    active_tab: GuiTab,
    command_tx: Sender<StreamCommand>,
    update_rx: Receiver<StreamUpdate>,
    worker: Option<JoinHandle<()>>,
}

impl StreamingStateRouter {
    pub fn new(store: Store) -> Self {
        let (command_tx, command_rx) = bounded(32);
        let (update_tx, update_rx) = bounded(32);
        let worker = std::thread::spawn(move || Self::worker_loop(command_rx, update_tx));
        Self {
            store,
            active_tab: GuiTab::Landing,
            command_tx,
            update_rx,
            worker: Some(worker),
        }
    }

    fn worker_loop(command_rx: Receiver<StreamCommand>, update_tx: Sender<StreamUpdate>) {
        for command in command_rx {
            match command {
                StreamCommand::ProcessEcg(ts) => {
                    let _ = update_tx.send(StreamUpdate::Ecg(ts.clone()));
                    let cfg = EcgPipelineConfig::default();
                    let result = run_beat_hrv_pipeline(&ts, &cfg);
                    let _ = update_tx.send(StreamUpdate::Events(result.events.clone()));
                    Self::publish_metrics(&result.events, result.fs, &update_tx);
                }
                StreamCommand::IngestEvents(ev, fs) => {
                    let _ = update_tx.send(StreamUpdate::Events(ev.clone()));
                    Self::publish_metrics(&ev, fs, &update_tx);
                }
                StreamCommand::Shutdown => break,
            }
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
            }
        }
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
        let _ = self.command_tx.send(StreamCommand::Shutdown);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}
