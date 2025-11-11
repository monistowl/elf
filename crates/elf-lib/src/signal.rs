use serde::{Deserialize, Serialize};

/// Basic typed time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeries {
    /// Uniform sampling frequency in Hz
    pub fs: f64,
    /// Samples
    pub data: Vec<f64>,
}

impl TimeSeries {
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn duration(&self) -> f64 {
        self.data.len() as f64 / self.fs
    }
}

/// Point events on a timeline (e.g., R-peaks indices)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Events {
    pub indices: Vec<usize>,
}

impl Events {
    pub fn from_indices(indices: Vec<usize>) -> Self {
        Self { indices }
    }
}

/// RR intervals (seconds)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RRSeries {
    pub rr: Vec<f64>,
}

impl RRSeries {
    pub fn from_events(events: &Events, fs: f64) -> Self {
        let mut rr = Vec::new();
        for w in events.indices.windows(2) {
            let dt = (w[1] as f64 - w[0] as f64) / fs;
            rr.push(dt);
        }
        Self { rr }
    }
}
