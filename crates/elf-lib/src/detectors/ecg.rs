use crate::signal::{Events, TimeSeries};

/// Very small starter R-peak detector (bandpass-free naive peak picking over moving average).
/// Replace with a proper Pan–Tompkins style pipeline as you iterate.
pub fn detect_r_peaks(ts: &TimeSeries, min_rr_s: f64) -> Events {
    let min_gap = (min_rr_s * ts.fs).max(1.0) as usize;
    let data = &ts.data;

    // simple moving average baseline
    let win = ((0.150 * ts.fs) as usize).max(1);
    let mut ma = vec![0.0; data.len()];
    let mut acc = 0.0;
    for i in 0..data.len() {
        acc += data[i];
        if i >= win {
            acc -= data[i - win];
        }
        ma[i] = acc / win as f64;
    }

    let mut peaks = Vec::new();
    let mut last_idx = 0usize;
    for i in 1..data.len() - 1 {
        let y = data[i] - ma[i];
        if y > 0.0 && y > (data[i - 1] - ma[i - 1]) && y > (data[i + 1] - ma[i + 1]) {
            if peaks.is_empty() || (i - last_idx) >= min_gap {
                peaks.push(i);
                last_idx = i;
            }
        }
    }
    Events::from_indices(peaks)
}
