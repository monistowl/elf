use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Axis {
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub width: f32,
    pub dash: Option<[f32; 2]>,
    pub color: Color,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Color(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineSeries {
    pub name: String,
    pub points: Vec<[f64; 2]>,
    pub style: Style,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Series {
    Line(LineSeries),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figure {
    pub title: Option<String>,
    pub x: Axis,
    pub y: Axis,
    pub series: Vec<Series>,
}

impl Figure {
    pub fn new(title: impl Into<Option<String>>) -> Self {
        Self {
            title: title.into(),
            x: Axis { label: None },
            y: Axis { label: None },
            series: Vec::new(),
        }
    }

    pub fn add_series(&mut self, series: Series) {
        self.series.push(series);
    }
}

pub trait PlotBackend {
    fn draw(&mut self, fig: &Figure) -> anyhow::Result<()>;
}

pub fn decimate_points(points: &[[f64; 2]], max_points: usize) -> Vec<[f64; 2]> {
    if points.len() <= max_points {
        return points.to_vec();
    }
    let bucket_size = points.len() as f64 / max_points as f64;
    let mut result = Vec::with_capacity(max_points);
    for i in 0..max_points {
        let start = (i as f64 * bucket_size).floor() as usize;
        if start >= points.len() {
            break;
        }
        let sample = points[start];
        result.push(sample);
    }
    result
}

pub fn figure_from_rr_limit(rr: &crate::signal::RRSeries, max_points: usize) -> Figure {
    let mut fig = Figure::new(Some("RR intervals".into()));
    let points: Vec<[f64; 2]> = rr
        .rr
        .iter()
        .enumerate()
        .map(|(i, value)| [i as f64, *value])
        .collect();
    let decimated = decimate_points(&points, max_points);
    fig.add_series(Series::Line(LineSeries {
        name: "RR".into(),
        points: decimated,
        style: Style {
            width: 2.0,
            dash: None,
            color: Color(0xFF0077),
        },
    }));
    fig
}

pub fn figure_from_rr(rr: &crate::signal::RRSeries) -> Figure {
    figure_from_rr_limit(rr, 1024)
}

pub fn figure_from_timeseries(
    title: &str,
    series: &crate::signal::TimeSeries,
    max_points: usize,
    color: u32,
) -> Figure {
    let dt = 1.0 / series.fs.max(1.0);
    let points: Vec<[f64; 2]> = series
        .data
        .iter()
        .enumerate()
        .map(|(i, value)| [i as f64 * dt, *value])
        .collect();
    let decimated = decimate_points(&points, max_points);
    let mut fig = Figure::new(Some(title.into()));
    fig.add_series(Series::Line(LineSeries {
        name: title.into(),
        points: decimated,
        style: Style {
            width: 1.4,
            dash: None,
            color: Color(color),
        },
    }));
    fig
}
