use crate::signal::{RRSeries, TimeSeries};
use realfft::RealFftPlanner;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SQIResult {
    pub kurtosis: f64,
    pub snr: f64,
    pub rr_cv: f64,
    pub spectral_entropy: f64,
    pub ppg_spike_ratio: f64,
}

impl SQIResult {
    pub fn is_acceptable(&self) -> bool {
        self.kurtosis >= 0.0 && self.snr >= 1.0 && self.rr_cv <= 0.2
    }
}

pub fn compute_kurtosis(ts: &TimeSeries) -> f64 {
    let data = &ts.data;
    let mean = data.iter().copied().sum::<f64>() / data.len() as f64;
    let m2 = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64;
    if m2 == 0.0 {
        return 0.0;
    }
    let m4 = data.iter().map(|x| (x - mean).powi(4)).sum::<f64>() / data.len() as f64;
    m4 / (m2 * m2)
}

pub fn compute_snr(ts: &TimeSeries) -> f64 {
    let window = 5.min(ts.data.len());
    if window == 0 {
        return 0.0;
    }
    let signal_power: f64 = ts.data.iter().map(|x| x * x).sum::<f64>() / ts.data.len() as f64;
    let mut noise_power = 0.0;
    for i in 0..ts.data.len() - window {
        let segment = &ts.data[i..i + window];
        let mean = segment.iter().copied().sum::<f64>() / window as f64;
        noise_power += segment.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / window as f64;
    }
    let noise = (noise_power / (ts.data.len() - window).max(1) as f64).max(1e-9);
    (signal_power / noise).max(0.0)
}

pub fn compute_rr_cv(rr: &RRSeries) -> f64 {
    if rr.rr.is_empty() {
        return 0.0;
    }
    let mean = rr.rr.iter().copied().sum::<f64>() / rr.rr.len() as f64;
    if mean == 0.0 {
        return 0.0;
    }
    let sd = (rr.rr.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rr.rr.len() as f64).sqrt();
    sd / mean
}

pub fn evaluate_sqi(ts: &TimeSeries, rr: &RRSeries) -> SQIResult {
    let kurtosis = compute_kurtosis(ts);
    let snr = compute_snr(ts);
    let rr_cv = compute_rr_cv(rr);
    let spectral_entropy = compute_spectral_entropy(ts);
    let ppg_spike_ratio = compute_ppg_spike_ratio(ts);
    SQIResult {
        kurtosis,
        snr,
        rr_cv,
        spectral_entropy,
        ppg_spike_ratio,
    }
}

pub fn compute_spectral_entropy(ts: &TimeSeries) -> f64 {
    let n = ts.data.len();
    if n == 0 {
        return 0.0;
    }
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut buffer = ts.data.clone();
    let mut spectrum = fft.make_output_vec();
    fft.process(&mut buffer, &mut spectrum).unwrap();
    let mut total_power = 0.0;
    let powers: Vec<f64> = spectrum
        .iter()
        .map(|c| {
            let p = c.norm_sqr();
            total_power += p;
            p
        })
        .collect();
    if total_power == 0.0 {
        return 0.0;
    }
    let mut entropy = 0.0;
    for power in powers {
        if power <= 0.0 {
            continue;
        }
        let p = power / total_power;
        entropy -= p * p.log2();
    }
    entropy
}

pub fn compute_ppg_spike_ratio(ts: &TimeSeries) -> f64 {
    if ts.data.len() < 2 {
        return 0.0;
    }
    let diffs: Vec<f64> = ts.data.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    let mean = diffs.iter().copied().sum::<f64>() / diffs.len() as f64;
    let sd = (diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / diffs.len() as f64).sqrt();
    if sd == 0.0 {
        return 0.0;
    }
    let threshold = mean + 2.0 * sd;
    let spikes = diffs.iter().filter(|&&d| d > threshold).count();
    spikes as f64 / diffs.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::TimeSeries;

    #[test]
    fn kurtosis_positive() {
        let ts = TimeSeries {
            fs: 250.0,
            data: vec![1.0, 1.0, 1.0],
        };
        assert!(compute_kurtosis(&ts) >= 0.0);
    }

    #[test]
    fn rr_cv_zero_when_constant() {
        let rr = RRSeries {
            rr: vec![1.0, 1.0, 1.0],
        };
        assert!((compute_rr_cv(&rr)).abs() < 1e-6);
    }

    #[test]
    fn spectral_entropy_non_negative() {
        let ts = TimeSeries {
            fs: 250.0,
            data: vec![1.0, 2.0, 3.0, 4.0],
        };
        assert!(compute_spectral_entropy(&ts) >= 0.0);
    }

    #[test]
    fn ppg_spike_ratio_zero_for_flat_signal() {
        let ts = TimeSeries {
            fs: 250.0,
            data: vec![1.0; 10],
        };
        assert!((compute_ppg_spike_ratio(&ts)).abs() < 1e-9);
    }
}
