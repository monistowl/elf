use std::{
    collections::HashMap,
    io::{self, Stdout},
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use elf_lib::{
    detectors::ecg::{run_beat_hrv_pipeline, EcgPipelineConfig},
    metrics::hrv::{hrv_nonlinear, hrv_psd, HRVNonlinear, HRVPsd, HRVTime},
    signal::TimeSeries,
};
use elf_run::{read_events_tsv, read_manifest, EventRow, RunManifest};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};

fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::default();
    let tick_rate = Duration::from_millis(150);
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| draw(f, &mut app))?;
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key)?;
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    restore_terminal()?;
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend).context("initializing terminal")
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Tab {
    Landing,
    Hrv,
    RunBundle,
}

impl Tab {
    fn title(&self) -> &'static str {
        match self {
            Tab::Landing => "Landing",
            Tab::Hrv => "ECG / HRV",
            Tab::RunBundle => "Run bundle",
        }
    }

    fn all() -> [Tab; 3] {
        [Tab::Landing, Tab::Hrv, Tab::RunBundle]
    }

    fn next(self) -> Self {
        match self {
            Tab::Landing => Tab::Hrv,
            Tab::Hrv => Tab::RunBundle,
            Tab::RunBundle => Tab::Landing,
        }
    }

    fn prev(self) -> Self {
        match self {
            Tab::Landing => Tab::RunBundle,
            Tab::Hrv => Tab::Landing,
            Tab::RunBundle => Tab::Hrv,
        }
    }

    fn index(self) -> usize {
        match self {
            Tab::Landing => 0,
            Tab::Hrv => 1,
            Tab::RunBundle => 2,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Focus {
    None,
    HrvPath,
    HrvFs,
    HrvMinRr,
    HrvInterp,
    HrvThreshold,
    BundlePath,
}

#[derive(Default)]
struct App {
    tab: Tab,
    focus: Focus,
    status: String,
    hrv: HrvState,
    bundle: BundleState,
    should_quit: bool,
}

#[derive(Default)]
struct TextField {
    value: String,
    cursor: usize,
}

impl TextField {
    fn new(default: &str) -> Self {
        Self {
            value: default.to_string(),
            cursor: default.len(),
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
                true
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
                true
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
                true
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                true
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
                true
            }
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.value.len();
                true
            }
            _ => false,
        }
    }
}

#[derive(Default)]
struct HrvState {
    path: TextField,
    fs: TextField,
    min_rr: TextField,
    interp_fs: TextField,
    threshold: TextField,
    result: Option<HrvOutcome>,
    error: Option<String>,
}

#[derive(Default)]
struct BundleState {
    path: TextField,
    summary: Option<RunBundleSummary>,
    error: Option<String>,
}

struct HrvOutcome {
    fs: f64,
    sample_count: usize,
    duration: f64,
    threshold_scale: f64,
    time: HRVTime,
    psd: HRVPsd,
    nonlinear: HRVNonlinear,
    rr_min: f64,
    rr_max: f64,
    rr_mean: f64,
    rr_count: usize,
}

struct RunBundleSummary {
    manifest: RunManifest,
    events: Vec<EventRow>,
    counts: HashMap<String, usize>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            tab: Tab::Landing,
            focus: Focus::None,
            status: "Use ←/→ or 1-3 to switch tabs. Enter runs the active action.".into(),
            hrv: HrvState {
                path: TextField::new("test_data/sample_ecg.txt"),
                fs: TextField::new("250"),
                min_rr: TextField::new("0.12"),
                interp_fs: TextField::new("4.0"),
                threshold: TextField::new("0.6"),
                result: None,
                error: None,
            },
            bundle: BundleState {
                path: TextField::new("test_data/run_bundle"),
                summary: None,
                error: None,
            },
            should_quit: false,
        }
    }
}

impl App {
    fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Left => self.tab = self.tab.prev(),
            KeyCode::Right => self.tab = self.tab.next(),
            KeyCode::Char('1') => self.tab = Tab::Landing,
            KeyCode::Char('2') => self.tab = Tab::Hrv,
            KeyCode::Char('3') => self.tab = Tab::RunBundle,
            KeyCode::Tab | KeyCode::Down => self.advance_focus(),
            KeyCode::Up => self.rewind_focus(),
            KeyCode::Enter => {
                if let Err(err) = self.run_action() {
                    self.status = format!("Error: {}", err);
                }
            }
            _ => {
                self.route_input(&key);
            }
        }
        Ok(())
    }

    fn advance_focus(&mut self) {
        self.focus = match (self.tab, self.focus) {
            (Tab::Hrv, Focus::HrvPath) => Focus::HrvFs,
            (Tab::Hrv, Focus::HrvFs) => Focus::HrvMinRr,
            (Tab::Hrv, Focus::HrvMinRr) => Focus::HrvInterp,
            (Tab::Hrv, Focus::HrvInterp) => Focus::HrvThreshold,
            (Tab::Hrv, _) => Focus::HrvPath,
            (Tab::RunBundle, Focus::BundlePath) => Focus::BundlePath,
            (Tab::RunBundle, _) => Focus::BundlePath,
            _ => Focus::None,
        };
    }

    fn rewind_focus(&mut self) {
        self.focus = match (self.tab, self.focus) {
            (Tab::Hrv, Focus::HrvThreshold) => Focus::HrvInterp,
            (Tab::Hrv, Focus::HrvInterp) => Focus::HrvMinRr,
            (Tab::Hrv, Focus::HrvMinRr) => Focus::HrvFs,
            (Tab::Hrv, Focus::HrvFs) => Focus::HrvPath,
            (Tab::Hrv, _) => Focus::HrvThreshold,
            (Tab::RunBundle, _) => Focus::BundlePath,
            _ => Focus::None,
        };
    }

    fn route_input(&mut self, key: &KeyEvent) {
        match (self.tab, self.focus) {
            (Tab::Hrv, Focus::HrvPath) => {
                if self.hrv.path.handle_key(key) {
                    self.hrv.error = None;
                }
            }
            (Tab::Hrv, Focus::HrvFs) => {
                if self.hrv.fs.handle_key(key) {
                    self.hrv.error = None;
                }
            }
            (Tab::Hrv, Focus::HrvMinRr) => {
                if self.hrv.min_rr.handle_key(key) {
                    self.hrv.error = None;
                }
            }
            (Tab::Hrv, Focus::HrvInterp) => {
                if self.hrv.interp_fs.handle_key(key) {
                    self.hrv.error = None;
                }
            }
            (Tab::Hrv, Focus::HrvThreshold) => {
                if self.hrv.threshold.handle_key(key) {
                    self.hrv.error = None;
                }
            }
            (Tab::RunBundle, Focus::BundlePath) => {
                if self.bundle.path.handle_key(key) {
                    self.bundle.error = None;
                }
            }
            _ => {}
        }
    }

    fn run_action(&mut self) -> Result<()> {
        match self.tab {
            Tab::Hrv => self.run_hrv_pipeline(),
            Tab::RunBundle => self.load_bundle(),
            Tab::Landing => Ok(()),
        }
    }

    fn run_hrv_pipeline(&mut self) -> Result<()> {
        let outcome = (|| -> Result<HrvOutcome> {
            let path = PathBuf::from(self.hrv.path.value.trim());
            let fs: f64 = self
                .hrv
                .fs
                .value
                .trim()
                .parse()
                .context("fs must be numeric")?;
            let min_rr: f64 = self
                .hrv
                .min_rr
                .value
                .trim()
                .parse()
                .context("min_rr (s) must be numeric")?;
            let interp_fs: f64 = self
                .hrv
                .interp_fs
                .value
                .trim()
                .parse()
                .context("interp fs must be numeric")?;
            let threshold: f64 = self
                .hrv
                .threshold
                .value
                .trim()
                .parse()
                .context("threshold scale must be numeric")?;
            let samples = text_series(&path)?;
            let ts = TimeSeries { fs, data: samples };
            let mut cfg = EcgPipelineConfig::default();
            cfg.min_rr_s = min_rr;
            cfg.threshold_scale = threshold;
            let result = run_beat_hrv_pipeline(&ts, &cfg);
            let psd = hrv_psd(&result.rr, interp_fs);
            let nonlinear = hrv_nonlinear(&result.rr);
            let rr_stats = summarize_rr(&result.rr.rr);
            Ok(HrvOutcome {
                fs: result.fs,
                sample_count: result.sample_count,
                duration: ts.duration(),
                threshold_scale: threshold,
                time: result.hrv,
                psd,
                nonlinear,
                rr_min: rr_stats.0,
                rr_max: rr_stats.1,
                rr_mean: rr_stats.2,
                rr_count: result.rr.rr.len(),
            })
        })();

        match outcome {
            Ok(outcome) => {
                let beat_count = outcome.rr_count;
                let sample_count = outcome.sample_count;
                let fs = outcome.fs;
                self.hrv.result = Some(outcome);
                self.hrv.error = None;
                self.status = format!(
                    "Loaded {} samples @ {:.1} Hz → {} beats",
                    sample_count, fs, beat_count
                );
                Ok(())
            }
            Err(err) => {
                self.hrv.error = Some(err.to_string());
                Err(err)
            }
        }
    }

    fn load_bundle(&mut self) -> Result<()> {
        let summary = (|| -> Result<RunBundleSummary> {
            let path = PathBuf::from(self.bundle.path.value.trim());
            if path.as_os_str().is_empty() {
                return Err(anyhow!("bundle directory is required"));
            }
            let manifest_path = path.join("run.json");
            let events_path = path.join("events.tsv");
            let manifest = read_manifest(&manifest_path)?;
            let events = read_events_tsv(&events_path)?;
            let counts = events.iter().fold(HashMap::new(), |mut acc, evt| {
                *acc.entry(evt.event_type.clone()).or_insert(0) += 1;
                acc
            });
            Ok(RunBundleSummary {
                manifest,
                events,
                counts,
            })
        })();

        match summary {
            Ok(summary) => {
                let event_count = summary.events.len();
                let description = format!(
                    "Loaded run bundle with {} events ({})",
                    event_count, summary.manifest.task
                );
                self.bundle.summary = Some(summary);
                self.bundle.error = None;
                self.status = description;
                Ok(())
            }
            Err(err) => {
                self.bundle.error = Some(err.to_string());
                Err(err)
            }
        }
    }
}

fn summarize_rr(rr: &[f64]) -> (f64, f64, f64) {
    if rr.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    let mut sum = 0.0;
    for &val in rr {
        min = min.min(val);
        max = max.max(val);
        sum += val;
    }
    let mean = sum / rr.len() as f64;
    (min, max, mean)
}

fn draw<B: Backend>(f: &mut Frame<'_, B>, app: &mut App) {
    let size = f.size();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);
    draw_tabs(f, layout[0], app);
    match app.tab {
        Tab::Landing => draw_landing(f, layout[1]),
        Tab::Hrv => draw_hrv(f, layout[1], app),
        Tab::RunBundle => draw_run_bundle(f, layout[1], app),
    }
    draw_status(f, layout[2], app);
}

fn draw_tabs<B: Backend>(f: &mut Frame<'_, B>, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.title())).collect();
    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("ELF TUI (ratatui)"),
        );
    f.render_widget(tabs, area);
}

fn draw_landing<B: Backend>(f: &mut Frame<'_, B>, area: Rect) {
    let text = vec![
        Line::from("Welcome to elf-tui"),
        Line::from("- Navigate with ←/→ arrows or 1-3."),
        Line::from("- Press Tab to cycle editable fields."),
        Line::from("- Enter runs the active tab action."),
        Line::from("- Press q to exit."),
        Line::from(""),
        Line::from(
            "This TUI mirrors the elf-gui flow: load ECG signals, compute HRV, and inspect run bundles.",
        ),
        Line::from(
            "Ratatui widgets drive the layout so the dashboard works in constrained terminals without a GPU.",
        ),
    ];
    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title("Landing"));
    f.render_widget(paragraph, area);
}

fn draw_hrv<B: Backend>(f: &mut Frame<'_, B>, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(42), Constraint::Min(0)])
        .split(area);
    draw_hrv_inputs(f, chunks[0], app);
    draw_hrv_results(f, chunks[1], app);
}

fn draw_hrv_inputs<B: Backend>(f: &mut Frame<'_, B>, area: Rect, app: &mut App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Min(0),
        ])
        .split(area);

    render_input(
        f,
        rows[0],
        "ECG samples (txt)",
        &app.hrv.path,
        app.focus == Focus::HrvPath,
    );
    render_input(
        f,
        rows[1],
        "fs (Hz)",
        &app.hrv.fs,
        app.focus == Focus::HrvFs,
    );
    render_input(
        f,
        rows[2],
        "min RR (s)",
        &app.hrv.min_rr,
        app.focus == Focus::HrvMinRr,
    );
    render_input(
        f,
        rows[3],
        "PSD interp fs",
        &app.hrv.interp_fs,
        app.focus == Focus::HrvInterp,
    );
    render_input(
        f,
        rows[4],
        "Threshold scale",
        &app.hrv.threshold,
        app.focus == Focus::HrvThreshold,
    );

    let mut lines = vec![
        Line::from("Press Enter to run the beat → HRV pipeline."),
        Line::from("Inputs mirror elf-gui defaults (Pan–Tompkins, PSD @ 4 Hz)."),
    ];
    if let Some(err) = &app.hrv.error {
        lines.push(Line::from(Span::styled(
            format!("Error: {}", err),
            Style::default().fg(Color::Red),
        )));
    }
    let hint = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title("Pipeline"));
    f.render_widget(hint, rows[5]);
}

fn draw_hrv_results<B: Backend>(f: &mut Frame<'_, B>, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title("HRV metrics");
    if let Some(result) = &app.hrv.result {
        let beats = format!(
            "{} beats, {:.2}–{:.2} s (mean {:.2})",
            result.rr_count, result.rr_min, result.rr_max, result.rr_mean
        );
        let lines = vec![
            Line::from(format!(
                "Samples: {} @ {:.1} Hz ({:.1}s)",
                result.sample_count, result.fs, result.duration
            )),
            Line::from(beats),
            Line::from(format!(
                "AVNN {:.3}s | SDNN {:.3}s | RMSSD {:.3}s | pNN50 {:.2}%",
                result.time.avnn,
                result.time.sdnn,
                result.time.rmssd,
                result.time.pnn50 * 100.0
            )),
            Line::from(format!(
                "PSD (LF/HF/VLF): {:.3} / {:.3} / {:.3} — LF/HF {:.2}",
                result.psd.lf, result.psd.hf, result.psd.vlf, result.psd.lf_hf
            )),
            Line::from(format!(
                "Nonlinear: SD1 {:.3}, SD2 {:.3}, SampEn {:.3}, DFA α1 {:.3}",
                result.nonlinear.sd1,
                result.nonlinear.sd2,
                result.nonlinear.samp_entropy,
                result.nonlinear.dfa_alpha1
            )),
            Line::from(format!(
                "Threshold scale {:.2} | PSD points {}",
                result.threshold_scale,
                result.psd.points.len()
            )),
        ];
        let body = Paragraph::new(lines).wrap(Wrap { trim: true }).block(block);
        f.render_widget(body, area);
    } else {
        let body = Paragraph::new("Run the pipeline to populate HRV metrics.")
            .wrap(Wrap { trim: true })
            .block(block);
        f.render_widget(body, area);
    }
}

fn draw_run_bundle<B: Backend>(f: &mut Frame<'_, B>, area: Rect, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);
    render_input(
        f,
        layout[0],
        "Bundle directory",
        &app.bundle.path,
        app.focus == Focus::BundlePath,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Run manifest + events");
    if let Some(summary) = &app.bundle.summary {
        let manifest_lines = vec![
            Line::from(format!(
                "Task {} (sub-{}, ses-{}, run-{})",
                summary.manifest.task,
                summary.manifest.sub,
                summary.manifest.ses,
                summary.manifest.run
            )),
            Line::from(format!(
                "Trials {} | Events {} | ISI {:.0} ms (+/- {:?})",
                summary.manifest.total_trials,
                summary.manifest.total_events,
                summary.manifest.isi_ms,
                summary.manifest.isi_jitter_ms
            )),
            Line::from(format!(
                "Randomization: {:?} | Seed: {:?}",
                summary.manifest.randomization_policy, summary.manifest.seed
            )),
        ];
        let mut counts: Vec<(&String, &usize)> = summary.counts.iter().collect();
        counts.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let counts: Vec<ListItem> = counts
            .into_iter()
            .map(|(evt, count)| ListItem::new(format!("{} — {}", evt, count)))
            .collect();
        let counts_block = List::new(counts)
            .block(Block::default().borders(Borders::ALL).title("Event types"))
            .highlight_style(Style::default().fg(Color::Cyan));
        let lines = Paragraph::new(manifest_lines)
            .block(Block::default().borders(Borders::ALL).title("Manifest"))
            .wrap(Wrap { trim: true });
        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(layout[1]);
        f.render_widget(lines, inner[0]);
        f.render_widget(counts_block, inner[1]);
    } else {
        let mut message = vec![
            Line::from("Point at a run bundle directory containing run.json + events.tsv."),
            Line::from("Press Enter to load."),
        ];
        if let Some(err) = &app.bundle.error {
            message.push(Line::from(Span::styled(
                format!("Error: {}", err),
                Style::default().fg(Color::Red),
            )));
        }
        let paragraph = Paragraph::new(message)
            .block(block)
            .wrap(Wrap { trim: true });
        f.render_widget(paragraph, layout[1]);
    }
}

fn draw_status<B: Backend>(f: &mut Frame<'_, B>, area: Rect, app: &App) {
    let status = Paragraph::new(app.status.as_str())
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    f.render_widget(status, area);
}

fn render_input<B: Backend>(
    f: &mut Frame<'_, B>,
    area: Rect,
    label: &str,
    field: &TextField,
    focused: bool,
) {
    let style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let paragraph = Paragraph::new(field.value.as_str())
        .style(style)
        .block(Block::default().borders(Borders::ALL).title(label));
    f.render_widget(paragraph, area);
    if focused {
        let cursor_x = area.x + 1 + field.cursor as u16;
        let cursor_y = area.y + 1;
        f.set_cursor(cursor_x.min(area.right().saturating_sub(1)), cursor_y);
    }
}

fn text_series(path: &std::path::Path) -> Result<Vec<f64>> {
    if !path.exists() {
        return Err(anyhow!("{} does not exist", path.display()));
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading samples from {}", path.display()))?;
    elf_lib::io::text::parse_f64_series(&content)
}
