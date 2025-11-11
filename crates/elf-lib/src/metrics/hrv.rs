use crate::signal::RRSeries;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HRVTime {
    pub n: usize,
    pub avnn: f64,
    pub sdnn: f64,
    pub rmssd: f64,
    pub pnn50: f64,
}

pub fn hrv_time(rr: &RRSeries) -> HRVTime {
    let n = rr.rr.len();
    let avnn = if n > 0 {
        rr.rr.iter().sum::<f64>() / n as f64
    } else {
        0.0
    };
    let sdnn = if n > 1 {
        let mean = avnn;
        (rr.rr.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0)).sqrt()
    } else {
        0.0
    };
    let rmssd = if n > 1 {
        let diffs = rr.rr.windows(2).map(|w| (w[1] - w[0]).powi(2));
        (diffs.sum::<f64>() / (n as f64 - 1.0)).sqrt()
    } else {
        0.0
    };
    let pnn50 = if n > 1 {
        let count = rr
            .rr
            .windows(2)
            .filter(|w| (w[1] - w[0]).abs() > 0.050)
            .count();
        (count as f64) / (n as f64 - 1.0)
    } else {
        0.0
    };

    HRVTime {
        n,
        avnn,
        sdnn,
        rmssd,
        pnn50,
    }
}
