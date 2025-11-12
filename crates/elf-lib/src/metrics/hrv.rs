use crate::signal::RRSeries;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HRVTime {
    pub n: usize,
    pub avnn: f64,
    pub sdnn: f64,
    pub rmssd: f64,
    pub pnn50: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HRVPsd {
    pub lf: f64,
    pub hf: f64,
    pub vlf: f64,
    pub lf_hf: f64,
    pub total_power: f64,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HRVNonlinear {
    pub sd1: f64,
    pub sd2: f64,
    pub samp_entropy: f64,
    pub dfa_alpha1: f64,
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

pub fn hrv_psd(rr: &RRSeries, fs_interp: f64) -> HRVPsd {
    let (freqs, powers) = welch_psd(rr, fs_interp);
    let total_power: f64 = powers.iter().sum();
    let lf_range = (0.04, 0.15);
    let hf_range = (0.15, 0.4);
    let vlf_range = (0.003, 0.04);
    let lf = integrate_band(&freqs, &powers, lf_range);
    let hf = integrate_band(&freqs, &powers, hf_range);
    let vlf = integrate_band(&freqs, &powers, vlf_range);
    let lf_hf = if hf > 0.0 { lf / hf } else { 0.0 };
    HRVPsd {
        lf,
        hf,
        vlf,
        lf_hf,
        total_power,
        points: freqs
            .into_iter()
            .zip(powers.into_iter())
            .map(|(f, p)| [f, p])
            .collect(),
    }
}

pub fn hrv_nonlinear(rr: &RRSeries) -> HRVNonlinear {
    let sd1 = poincare_sd1(rr);
    let sdnn = if rr.rr.len() > 1 {
        let mean = rr.rr.iter().sum::<f64>() / rr.rr.len() as f64;
        (rr.rr.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (rr.rr.len() as f64 - 1.0)).sqrt()
    } else {
        0.0
    };
    let sd2 = (2.0 * sdnn * sdnn - sd1 * sd1).max(0.0).sqrt();
    let samp_entropy = sample_entropy(&rr.rr, 2, 0.2 * sdnn.max(0.0001));
    let dfa_alpha1 = detrended_fluctuation_alpha1(&rr.rr);
    HRVNonlinear {
        sd1,
        sd2,
        samp_entropy,
        dfa_alpha1,
    }
}

fn sample_entropy(data: &[f64], m: usize, r: f64) -> f64 {
    if data.len() <= m + 1 {
        return 0.0;
    }
    let mut count_m = 0f64;
    let mut count_m1 = 0f64;
    for i in 0..data.len() - m {
        for j in (i + 1)..data.len() - m {
            if max_diff(data, i, j, m) < r {
                count_m += 1.0;
                if (j + m) < data.len() && max_diff(data, i, j, m + 1) < r {
                    count_m1 += 1.0;
                }
            }
        }
    }
    if count_m1 == 0.0 || count_m == 0.0 {
        0.0
    } else {
        -(count_m1 / count_m).ln()
    }
}

fn max_diff(data: &[f64], i: usize, j: usize, length: usize) -> f64 {
    data[i..i + length]
        .iter()
        .zip(data[j..j + length].iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max)
}

fn poincare_sd1(rr: &RRSeries) -> f64 {
    let diffs: Vec<f64> = rr.rr.windows(2).map(|w| w[1] - w[0]).collect();
    if diffs.is_empty() {
        return 0.0;
    }
    let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
    let var = diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / diffs.len() as f64;
    (0.5 * var).sqrt()
}

fn detrended_fluctuation_alpha1(rr: &[f64]) -> f64 {
    const MIN_WINDOW: usize = 4;
    const MAX_WINDOW: usize = 16;
    if rr.len() < MIN_WINDOW * 2 {
        return 0.0;
    }
    let mean = rr.iter().copied().sum::<f64>() / rr.len() as f64;
    let mut profile = Vec::with_capacity(rr.len());
    let mut acc = 0.0;
    for &value in rr {
        acc += value - mean;
        profile.push(acc);
    }
    let max_window = rr.len().min(MAX_WINDOW);
    if max_window < MIN_WINDOW {
        return 0.0;
    }
    let mut samples = Vec::new();
    for window in MIN_WINDOW..=max_window {
        let mut idx = 0;
        let mut total = 0.0;
        let mut segments = 0;
        while idx + window <= profile.len() {
            let segment = &profile[idx..idx + window];
            let (slope, intercept) = linear_fit(segment);
            let mut err = 0.0;
            for (i, &y) in segment.iter().enumerate() {
                let trend = slope * i as f64 + intercept;
                let diff = y - trend;
                err += diff * diff;
            }
            total += err / window as f64;
            segments += 1;
            idx += window;
        }
        if segments == 0 {
            continue;
        }
        let rms = (total / segments as f64).sqrt();
        if rms.is_finite() && rms > 0.0 {
            samples.push((window as f64, rms));
        }
    }
    if samples.len() < 2 {
        return 0.0;
    }
    log_log_slope(&samples)
}

fn linear_fit(segment: &[f64]) -> (f64, f64) {
    let n = segment.len();
    if n < 2 {
        let mean = segment.iter().copied().next().unwrap_or(0.0);
        return (0.0, mean);
    }
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_xy = 0.0;
    for (i, &y) in segment.iter().enumerate() {
        let x = i as f64;
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let n_f = n as f64;
    let denom = n_f * sum_xx - sum_x * sum_x;
    if denom.abs() < f64::EPSILON {
        let mean = sum_y / n_f;
        return (0.0, mean);
    }
    let slope = (n_f * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n_f;
    (slope, intercept)
}

fn log_log_slope(points: &[(f64, f64)]) -> f64 {
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xx = 0.0;
    let mut sum_xy = 0.0;
    let mut n = 0.0;
    for &(scale, rms) in points {
        if scale <= 0.0 || rms <= 0.0 {
            continue;
        }
        let x = scale.ln();
        let y = rms.ln();
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
        n += 1.0;
    }
    if n < 2.0 {
        return 0.0;
    }
    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < f64::EPSILON {
        return 0.0;
    }
    (n * sum_xy - sum_x * sum_y) / denom
}

fn integrate_band(freqs: &[f64], powers: &[f64], band: (f64, f64)) -> f64 {
    freqs
        .iter()
        .zip(powers)
        .filter(|(f, _)| **f >= band.0 && **f < band.1)
        .map(|(_, p)| *p)
        .sum()
}

fn welch_psd(rr: &RRSeries, fs_interp: f64) -> (Vec<f64>, Vec<f64>) {
    let signal = interpolate_rr(rr, fs_interp);
    let n = signal.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }
    let window = ((fs_interp * 30.0).max(4.0).min(n as f64)) as usize;
    let step = window / 2;
    let mut planner = RealFftPlanner::<f64>::new();
    let r2c = planner.plan_fft_forward(window);
    let mut freqs = Vec::new();
    let mut powers = Vec::new();
    let window_func: Vec<f64> = hann(window);
    let mut pos = 0;
    let mut segments = 0;
    while pos + window <= n {
        let slice = &signal[pos..pos + window];
        let mut frame: Vec<f64> = slice
            .iter()
            .zip(window_func.iter())
            .map(|(x, w)| x * w)
            .collect();
        let mut spectrum = r2c.make_output_vec();
        r2c.process(&mut frame, &mut spectrum).unwrap();
        let scale = 1.0 / window as f64;
        for (k, val) in spectrum.iter().enumerate() {
            if segments == 0 {
                freqs.push(k as f64 * fs_interp / window as f64);
                powers.push(0.0);
            }
            let power = if k == 0 || (window % 2 == 0 && k == window / 2) {
                val.norm_sqr()
            } else {
                2.0 * val.norm_sqr()
            } * scale;
            powers[k] += power;
        }
        segments += 1;
        pos += step;
    }
    if segments > 0 {
        for p in powers.iter_mut() {
            *p /= segments as f64;
        }
    }
    (freqs, powers)
}

fn interpolate_rr(rr: &RRSeries, fs: f64) -> Vec<f64> {
    let mut times = Vec::new();
    let mut acc = 0.0;
    for interval in &rr.rr {
        acc += interval;
        times.push(acc);
    }
    if times.is_empty() {
        return vec![];
    }
    let duration = *times.last().unwrap();
    let n = (duration * fs).ceil() as usize;
    let mut signal = Vec::with_capacity(n);
    let mut idx = 0;
    for i in 0..n {
        let t = i as f64 / fs;
        while idx + 1 < times.len() && times[idx] < t {
            idx += 1;
        }
        let delta = if idx == 0 { rr.rr[0] } else { rr.rr[idx] };
        let value = if delta == 0.0 { 60.0 } else { 60.0 / delta };
        signal.push(value);
    }
    signal
}

fn hann(size: usize) -> Vec<f64> {
    (0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / (size as f64)).cos()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::RRSeries;
    use serde::Deserialize;
    use std::{fs::File, path::PathBuf};

    fn rr_series() -> RRSeries {
        let data = [
            0.82, 0.78, 0.80, 0.79, 0.83, 0.77, 0.84, 0.88, 0.86, 0.81, 0.79, 0.82, 0.85, 0.78,
            0.80, 0.79, 0.83, 0.84, 0.82, 0.81,
        ];
        RRSeries { rr: data.to_vec() }
    }

    fn assert_close(actual: f64, expected: f64, rel_tol: f64) {
        let tol = expected.abs().max(1.0) * rel_tol;
        let diff = (actual - expected).abs();
        assert!(
            diff <= tol,
            "expected {expected}, got {actual} (diff {diff} > tol {tol})"
        );
    }

    #[test]
    fn welch_psd_regression_snapshot() {
        let rr = rr_series();
        let psd = hrv_psd(&rr, 4.0);
        assert!(psd.points.len() > 10);
        assert_close(psd.lf, 43426.124124153386, 1e-6);
        assert_close(psd.hf, 112.27098917234595, 1e-6);
        assert_close(psd.total_power, 131892.82018205707, 1e-6);
    }

    #[test]
    fn nonlinear_metrics_regression_snapshot() {
        let rr = rr_series();
        let metrics = hrv_nonlinear(&rr);
        assert_close(metrics.sd1, 0.02640511156760194, 1e-6);
        assert_close(metrics.sd2, 0.03157499975325861, 1e-6);
        assert_close(metrics.samp_entropy, 0.40546510810816444, 1e-6);
        assert_close(metrics.dfa_alpha1, 0.8558325863087242, 1e-6);
    }

    #[derive(Deserialize)]
    struct SyntheticExpected {
        rr: Vec<f64>,
    }

    fn synthetic_rr_series() -> RRSeries {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf();
        let path = root.join("test_data/synthetic_recording_a_expected.json");
        let file: SyntheticExpected = serde_json::from_reader(File::open(&path).unwrap())
            .expect("read synthetic expectations");
        RRSeries { rr: file.rr }
    }

    #[test]
    fn synthetic_recording_psd_snapshot() {
        let rr = synthetic_rr_series();
        let psd = hrv_psd(&rr, 4.0);
        assert_close(psd.lf, 25993.846050152697, 1e-6);
        assert_close(psd.hf, 78.40932384922252, 1e-6);
        assert_close(psd.total_power, 79178.15543679718, 1e-6);
    }

    #[test]
    fn synthetic_recording_nonlinear_snapshot() {
        let rr = synthetic_rr_series();
        let metrics = hrv_nonlinear(&rr);
        assert_close(metrics.sd1, 0.029848100289785514, 1e-6);
        assert_close(metrics.sd2, 0.035654061777196315, 1e-6);
        assert_close(metrics.samp_entropy, 0.0, 1e-6);
        assert_close(metrics.dfa_alpha1, 0.9329347133704009, 1e-6);
    }
}
