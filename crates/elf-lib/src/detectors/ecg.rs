use crate::{
    metrics::hrv::{hrv_time, HRVTime},
    signal::{Events, RRSeries, TimeSeries},
};
use serde::{Deserialize, Serialize};

/// Configurable parameters for the ECG beat detection + HRV pipeline.
#[derive(Debug, Clone, Copy)]
pub struct EcgPipelineConfig {
    /// Lower cutoff for the single-pole high-pass filter (Hz).
    pub lowcut_hz: f64,
    /// Upper cutoff for the single-pole low-pass filter (Hz).
    pub highcut_hz: f64,
    /// Moving window integration length (seconds).
    pub integration_window_s: f64,
    /// Minimum physiological RR distance / refractory period (seconds).
    pub min_rr_s: f64,
    /// Scale between noise and signal envelopes for the adaptive threshold.
    pub threshold_scale: f64,
    /// How far back to search (seconds) for the precise R-peak after a detection.
    pub search_back_s: f64,
}

impl Default for EcgPipelineConfig {
    fn default() -> Self {
        Self {
            lowcut_hz: 5.0,
            highcut_hz: 15.0,
            integration_window_s: 0.150,
            min_rr_s: 0.120,
            threshold_scale: 0.6,
            search_back_s: 0.150,
        }
    }
}

/// Combined result of the beat detection pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatHrvPipelineResult {
    pub fs: f64,
    pub sample_count: usize,
    pub events: Events,
    pub rr: RRSeries,
    pub hrv: HRVTime,
}

impl BeatHrvPipelineResult {
    pub fn from_events(ts: &TimeSeries, events: Events) -> Self {
        let rr = RRSeries::from_events(&events, ts.fs);
        let hrv = hrv_time(&rr);
        Self {
            fs: ts.fs,
            sample_count: ts.len(),
            events,
            rr,
            hrv,
        }
    }
}

/// Run the improved Pan–Tompkins-inspired pipeline with a minimal configuration surface.
pub fn detect_r_peaks(ts: &TimeSeries, min_rr_s: f64) -> Events {
    let mut cfg = EcgPipelineConfig::default();
    cfg.min_rr_s = min_rr_s.max(0.15);
    detect_r_peaks_with_config(ts, &cfg)
}

/// Detect R-peaks using the configurable pipeline.
pub fn detect_r_peaks_with_config(ts: &TimeSeries, cfg: &EcgPipelineConfig) -> Events {
    if ts.is_empty() {
        return Events::from_indices(Vec::new());
    }

    let (bandpassed, integrated) = pan_tompkins_envelope(ts, cfg);
    let peaks = pick_peaks(&bandpassed, &integrated, ts.fs, cfg);

    if peaks.len() < 2 {
        // Fall back to the earlier naive peak picker if the adaptive method underperformed.
        return Events::from_indices(fallback_peak_picker(ts, cfg));
    }

    Events::from_indices(peaks)
}

/// Convenience helper that runs R-peak detection, converts to RR intervals, and computes time-domain HRV.
pub fn run_beat_hrv_pipeline(ts: &TimeSeries, cfg: &EcgPipelineConfig) -> BeatHrvPipelineResult {
    let events = detect_r_peaks_with_config(ts, cfg);
    BeatHrvPipelineResult::from_events(ts, events)
}

fn pan_tompkins_envelope(ts: &TimeSeries, cfg: &EcgPipelineConfig) -> (Vec<f64>, Vec<f64>) {
    let data = &ts.data;
    let fs = ts.fs.max(1.0);
    let bandpassed = bandpass(data, fs, cfg.lowcut_hz, cfg.highcut_hz);
    let derivative = derivative(&bandpassed);
    let squared = square(&derivative);
    let win = ((cfg.integration_window_s * fs).round() as usize).max(1);
    let integrated = moving_average(&squared, win);
    (bandpassed, integrated)
}

fn bandpass(data: &[f64], fs: f64, low: f64, high: f64) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    let hp = if low > 0.0 {
        single_pole_highpass(data, fs, low)
    } else {
        data.to_vec()
    };
    if high <= 0.0 || high >= fs * 0.5 {
        hp
    } else {
        single_pole_lowpass(&hp, fs, high)
    }
}

fn single_pole_highpass(data: &[f64], fs: f64, cutoff: f64) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    let dt = 1.0 / fs;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff.max(0.01));
    let alpha = rc / (rc + dt);
    let mut out = Vec::with_capacity(data.len());
    let mut prev_y = data[0];
    let mut prev_x = data[0];
    for &x in data {
        let y = alpha * (prev_y + x - prev_x);
        out.push(y);
        prev_y = y;
        prev_x = x;
    }
    out
}

fn single_pole_lowpass(data: &[f64], fs: f64, cutoff: f64) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    let dt = 1.0 / fs;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff.max(0.01));
    let alpha = dt / (rc + dt);
    let mut out = Vec::with_capacity(data.len());
    let mut prev = data[0];
    for &x in data {
        prev = prev + alpha * (x - prev);
        out.push(prev);
    }
    out
}

fn derivative(data: &[f64]) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0.0; data.len()];
    for i in 1..data.len() {
        out[i] = data[i] - data[i - 1];
    }
    out
}

fn square(data: &[f64]) -> Vec<f64> {
    data.iter().map(|x| x * x).collect()
}

fn moving_average(data: &[f64], win: usize) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    if win <= 1 {
        return data.to_vec();
    }
    let mut out = vec![0.0; data.len()];
    let mut acc = 0.0;
    for (i, &sample) in data.iter().enumerate() {
        acc += sample;
        if i >= win {
            acc -= data[i - win];
        }
        out[i] = acc / win as f64;
    }
    out
}

fn pick_peaks(
    bandpassed: &[f64],
    envelope: &[f64],
    fs: f64,
    cfg: &EcgPipelineConfig,
) -> Vec<usize> {
    if bandpassed.is_empty() || envelope.is_empty() {
        return Vec::new();
    }

    let refractory = (cfg.min_rr_s * fs).round().clamp(1.0, f64::MAX) as usize;
    let search = (cfg.search_back_s * fs).round().max(1.0) as usize;

    let init = envelope.len().min((fs as usize).max(1));
    let avg = if init > 0 {
        envelope[..init].iter().sum::<f64>() / init as f64
    } else {
        0.0
    };
    let mut signal_level = avg;
    let mut noise_level = avg * 0.5;
    let mut threshold = noise_level + cfg.threshold_scale * (signal_level - noise_level).max(0.0);
    let mut last_peak_sample = 0usize;
    let mut peaks = Vec::new();

    for i in 0..envelope.len() {
        let sample = envelope[i];
        let refractory_ok = peaks.is_empty() || i - last_peak_sample >= refractory;
        if sample >= threshold && refractory_ok {
            let start = i.saturating_sub(search);
            let end = i.min(bandpassed.len() - 1);
            let mut idx = start;
            let mut max_val = f64::MIN;
            for j in start..=end {
                if bandpassed[j] > max_val {
                    max_val = bandpassed[j];
                    idx = j;
                }
            }
            peaks.push(idx);
            last_peak_sample = i;
            signal_level = 0.125 * sample + 0.875 * signal_level;
        } else {
            noise_level = 0.125 * sample + 0.875 * noise_level;
        }

        threshold = noise_level + cfg.threshold_scale * (signal_level - noise_level).max(0.0);
    }

    peaks.sort_unstable();
    peaks.dedup();
    peaks
}

fn fallback_peak_picker(ts: &TimeSeries, cfg: &EcgPipelineConfig) -> Vec<usize> {
    let min_gap = (cfg.min_rr_s * ts.fs).max(1.0) as usize;
    let data = &ts.data;
    if data.len() < 3 {
        return Vec::new();
    }

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
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::io::wfdb as wfdb_io;
    use std::path::PathBuf;

    #[test]
    fn detects_regular_beats() {
        let fs = 250.0;
        let rr = [0.82, 0.78, 0.8, 0.79, 0.81, 0.77, 0.84, 0.88];
        let ts = synthetic_timeseries(fs, &rr);
        let events = detect_r_peaks(&ts, 0.3);
        assert_eq!(events.indices.len(), rr.len() + 1);
    }

    #[test]
    fn beat_hrv_pipeline_returns_metrics() {
        let fs = 250.0;
        let rr = [0.9, 0.85, 0.88, 0.86, 0.82, 0.81, 0.8];
        let ts = synthetic_timeseries(fs, &rr);
        let cfg = EcgPipelineConfig::default();
        let result = run_beat_hrv_pipeline(&ts, &cfg);
        assert_eq!(result.events.indices.len(), rr.len() + 1);
        assert_eq!(result.rr.rr.len(), rr.len());
        assert!(result.hrv.rmssd > 0.0);
    }

    fn synthetic_timeseries(fs: f64, rr: &[f64]) -> TimeSeries {
        use std::f64::consts::PI;
        let mut beats = Vec::with_capacity(rr.len() + 1);
        let mut t = 0.5;
        beats.push(t);
        for &interval in rr {
            t += interval;
            beats.push(t);
        }
        let duration = beats.last().copied().unwrap_or(1.0) + 1.0;
        let samples = (duration * fs) as usize;
        let mut data = Vec::with_capacity(samples);
        for i in 0..samples {
            let time = i as f64 / fs;
            let mut v = 0.05 * (2.0 * PI * 1.0 * time).sin();
            for &bt in &beats {
                let width = 0.02;
                let amp = (-0.5 * ((time - bt) / width).powi(2)).exp();
                v += 1.2 * amp;
            }
            data.push(v);
        }
        TimeSeries { fs, data }
    }

    #[test]
    fn detector_matches_mitdb_118_annotations() {
        let root = workspace_root();
        let header = root.join("test_data/mitdb/118.hea");
        let annotations = root.join("test_data/mitdb/118.atr");
        let ts = wfdb_io::load_wfdb_lead(&header, 0).expect("load MIT-BIH lead");
        let detected = detect_r_peaks_with_config(&ts, &EcgPipelineConfig::default());
        let ann = wfdb_io::load_wfdb_events(&annotations).expect("load MIT-BIH annotations");
        let tolerance = ((0.04 * ts.fs).round() as usize).max(2);
        let matches = count_matches(&ann.indices, &detected.indices, tolerance);
        let coverage = matches as f64 / ann.indices.len() as f64;
        assert!(
            coverage >= 0.96,
            "detector coverage too low: {}/{} ({:.1}%)",
            matches,
            ann.indices.len(),
            coverage * 100.0
        );
        let false_positive = detected.indices.len().saturating_sub(matches);
        let max_false_positive = ((ann.indices.len() as f64) * 0.15).ceil() as usize;
        assert!(
            false_positive <= max_false_positive,
            "too many extra detections: {} (limit {})",
            false_positive,
            max_false_positive
        );
    }

    #[test]
    fn detector_matches_mitdb_205_annotations() {
        let root = workspace_root();
        let header = root.join("test_data/mitdb/205.hea");
        let annotations = root.join("test_data/mitdb/205.atr");
        let ts = wfdb_io::load_wfdb_lead(&header, 0).expect("load MIT-BIH lead");
        let detected = detect_r_peaks_with_config(&ts, &EcgPipelineConfig::default());
        let ann = wfdb_io::load_wfdb_events(&annotations).expect("load MIT-BIH annotations");
        let tolerance = ((0.04 * ts.fs).round() as usize).max(2);
        let matches = count_matches(&ann.indices, &detected.indices, tolerance);
        let coverage = matches as f64 / ann.indices.len() as f64;
        assert!(
            coverage >= 0.95,
            "detector coverage too low for 205: {}/{} ({:.1}%)",
            matches,
            ann.indices.len(),
            coverage * 100.0
        );
        let false_positive = detected.indices.len().saturating_sub(matches);
        let max_false_positive = ((ann.indices.len() as f64) * 0.2).ceil() as usize;
        assert!(
            false_positive <= max_false_positive,
            "too many extra detections for 205: {} (limit {})",
            false_positive,
            max_false_positive
        );
    }

    fn count_matches(ann: &[usize], det: &[usize], tol: usize) -> usize {
        if ann.is_empty() || det.is_empty() {
            return 0;
        }
        let mut matches = 0;
        let mut idx = 0;
        for &ann_sample in ann {
            while idx < det.len() && det[idx] + tol < ann_sample {
                idx += 1;
            }
            if idx < det.len() {
                let diff = (det[idx] as isize - ann_sample as isize).abs() as usize;
                if diff <= tol {
                    matches += 1;
                    continue;
                }
            }
            if idx > 0 {
                let prev = det[idx - 1];
                let diff = (prev as isize - ann_sample as isize).abs() as usize;
                if diff <= tol {
                    matches += 1;
                }
            }
        }
        matches
    }

    fn workspace_root() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf()
    }
}
