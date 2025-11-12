use elf_lib::{
    plot::{Color, Figure, LineSeries, Series, Style},
    signal::RRSeries,
};

pub fn average_rr(rr: &RRSeries) -> Option<f64> {
    if rr.rr.is_empty() {
        return None;
    }
    let sum = rr.rr.iter().copied().sum::<f64>();
    Some(sum / rr.rr.len() as f64)
}

pub fn heart_rate_from_rr(rr: &RRSeries) -> Option<f64> {
    average_rr(rr).and_then(|mean| if mean > 0.0 { Some(60.0 / mean) } else { None })
}

pub fn rr_histogram_figure(rr: &RRSeries, bins: usize) -> Option<Figure> {
    if rr.rr.is_empty() || bins == 0 {
        return None;
    }
    let min_rr = rr.rr.iter().copied().fold(f64::INFINITY, f64::min);
    let max_rr = rr.rr.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if (max_rr - min_rr).abs() < f64::EPSILON {
        return None;
    }
    let width = (max_rr - min_rr) / bins as f64;
    let mut counts = vec![0u32; bins];
    for &value in &rr.rr {
        let mut idx = ((value - min_rr) / width).floor() as usize;
        if idx >= bins {
            idx = bins - 1;
        }
        counts[idx] += 1;
    }
    let total = counts.iter().sum::<u32>() as f64;
    let points: Vec<[f64; 2]> = counts
        .iter()
        .enumerate()
        .map(|(i, &count)| {
            let bin_center = min_rr + width * (i as f64 + 0.5);
            [bin_center, count as f64 / total]
        })
        .collect();
    let mut fig = Figure::new(Some("RR histogram".to_string()));
    fig.add_series(Series::Line(LineSeries {
        name: "RR distr".into(),
        points,
        style: Style {
            width: 2.0,
            dash: None,
            color: Color(0xFFAA00),
        },
    }));
    Some(fig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use elf_lib::signal::RRSeries;

    #[test]
    fn average_rr_returns_mean() {
        let rr = RRSeries {
            rr: vec![1.0, 1.5, 2.0],
        };
        assert_eq!(average_rr(&rr), Some(4.5 / 3.0));
    }

    #[test]
    fn heart_rate_from_rr_invalid_mean() {
        let rr = RRSeries { rr: vec![] };
        assert_eq!(heart_rate_from_rr(&rr), None);
    }

    #[test]
    fn histogram_requires_bins() {
        let rr = RRSeries {
            rr: vec![1.0, 1.1, 1.2, 1.3],
        };
        assert!(rr_histogram_figure(&rr, 4).is_some());
    }
}
