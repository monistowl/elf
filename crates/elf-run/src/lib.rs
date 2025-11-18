use anyhow::{Context, Result};
use csv::{ReaderBuilder, Trim, WriterBuilder};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize, Clone)]
pub struct DesignSpec {
    pub name: String,
    #[serde(default)]
    pub timing: Option<TimingSpec>,
    #[serde(default)]
    pub randomization: Option<RandomizationSpec>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct TimingSpec {
    #[serde(default)]
    pub isi_ms: Option<f64>,
    #[serde(default)]
    pub response_deadline_ms: Option<f64>,
    #[serde(default)]
    pub isi_jitter_ms: Option<f64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct RandomizationSpec {
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TrialRow {
    #[serde(default)]
    pub trial: usize,
    #[serde(default)]
    pub block: Option<usize>,
    #[serde(default)]
    pub stim_id: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    pub duration_ms: f64,
    #[serde(default)]
    pub resp_key: Option<String>,
    #[serde(default)]
    pub resp_rt_ms: Option<f64>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TrialSpec {
    pub trial: usize,
    pub block: usize,
    pub stim_id: String,
    pub condition: String,
    pub duration_ms: f64,
    pub resp_key: Option<String>,
    pub resp_rt_ms: Option<f64>,
    pub value: Option<String>,
}

impl TrialSpec {
    fn from_row(row: TrialRow, index: usize) -> Self {
        let trial = if row.trial == 0 { index + 1 } else { row.trial };
        Self {
            trial,
            block: row.block.unwrap_or(1),
            stim_id: row
                .stim_id
                .unwrap_or_else(|| format!("trial-{}", trial))
                .replace(' ', "-"),
            condition: row.condition.unwrap_or_else(|| "".into()),
            duration_ms: row.duration_ms,
            resp_key: row.resp_key,
            resp_rt_ms: row.resp_rt_ms,
            value: row.value,
        }
    }
}

#[test]
fn randomization_block_shuffle_changes_order() {
    let design = DesignSpec {
        name: "shuffle".into(),
        timing: None,
        randomization: Some(RandomizationSpec {
            policy: Some("block-shuffle".into()),
            seed: Some(42),
        }),
    };
    let trials = vec![
        TrialSpec {
            trial: 1,
            block: 1,
            stim_id: "a".into(),
            condition: "a".into(),
            duration_ms: 100.0,
            resp_key: None,
            resp_rt_ms: None,
            value: None,
        },
        TrialSpec {
            trial: 2,
            block: 1,
            stim_id: "b".into(),
            condition: "b".into(),
            duration_ms: 100.0,
            resp_key: None,
            resp_rt_ms: None,
            value: None,
        },
        TrialSpec {
            trial: 3,
            block: 2,
            stim_id: "c".into(),
            condition: "c".into(),
            duration_ms: 100.0,
            resp_key: None,
            resp_rt_ms: None,
            value: None,
        },
    ];
    let bundle = simulate_run(&design, &trials, "01", "01", "01");
    assert_eq!(bundle.manifest.total_trials, 3);
    assert_eq!(
        bundle.manifest.randomization_policy.as_deref(),
        Some("block-shuffle")
    );
    // block shuffle should keep block 1 trials grouped but permute within block
    let first_block: Vec<_> = bundle
        .events
        .iter()
        .filter(|e| e.block == 1 && e.event_type == "stim")
        .map(|e| e.stim_id.as_str())
        .collect();
    assert_eq!(first_block.len(), 2);
    assert_ne!(first_block[0], "a");
    assert_ne!(first_block[1], "b");
}

#[test]
fn manifest_records_jitter() {
    let design = DesignSpec {
        name: "jitter".into(),
        timing: Some(TimingSpec {
            isi_ms: Some(500.0),
            response_deadline_ms: None,
            isi_jitter_ms: Some(100.0),
        }),
        randomization: None,
    };
    let trials = vec![TrialSpec {
        trial: 1,
        block: 1,
        stim_id: "x".into(),
        condition: "x".into(),
        duration_ms: 100.0,
        resp_key: None,
        resp_rt_ms: None,
        value: None,
    }];
    let bundle = simulate_run(&design, &trials, "01", "01", "02");
    assert_eq!(bundle.manifest.isi_ms, 500.0);
    assert_eq!(bundle.manifest.isi_jitter_ms, Some(100.0));
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventRow {
    pub onset: f64,
    pub duration: f64,
    pub trial: usize,
    pub block: usize,
    pub event_type: String,
    pub stim_id: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub resp_key: Option<String>,
    #[serde(default)]
    pub resp_rt: Option<f64>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RunManifest {
    pub sub: String,
    pub ses: String,
    pub run: String,
    pub task: String,
    pub design: String,
    pub total_trials: usize,
    pub total_events: usize,
    pub seed: Option<u64>,
    pub randomization_policy: Option<String>,
    pub isi_ms: f64,
    pub isi_jitter_ms: Option<f64>,
    pub start_time_unix: f64,
}

pub struct RunBundle {
    pub events: Vec<EventRow>,
    pub manifest: RunManifest,
}

fn shuffle_trials(trials: &mut [TrialSpec], spec: &RandomizationSpec, rng: &mut StdRng) {
    match spec.policy.as_deref() {
        Some("block-shuffle") => shuffle_by_block(trials, rng),
        _ => trials.shuffle(rng),
    }
}

fn shuffle_by_block(trials: &mut [TrialSpec], rng: &mut StdRng) {
    let mut start = 0;
    while start < trials.len() {
        let block = trials[start].block;
        let mut end = start + 1;
        while end < trials.len() && trials[end].block == block {
            end += 1;
        }
        trials[start..end].shuffle(rng);
        start = end;
    }
}

pub fn read_design(path: &Path) -> Result<DesignSpec> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read design {}", path.display()))?;
    let design: DesignSpec =
        toml::from_str(&contents).with_context(|| format!("parsing design {}", path.display()))?;
    Ok(design)
}

pub fn read_trials(path: &Path) -> Result<Vec<TrialSpec>> {
    let mut reader = ReaderBuilder::new()
        .trim(Trim::All)
        .from_path(path)
        .with_context(|| format!("opening trials {}", path.display()))?;
    let mut trials = Vec::new();
    for (idx, row) in reader.deserialize::<TrialRow>().enumerate() {
        let row = row.with_context(|| format!("parsing trial row {}", idx + 1))?;
        trials.push(TrialSpec::from_row(row, idx));
    }
    Ok(trials)
}

pub fn simulate_run(
    design: &DesignSpec,
    trials: &[TrialSpec],
    sub: &str,
    ses: &str,
    run_id: &str,
) -> RunBundle {
    let randomization = design.randomization.clone();
    let seed = randomization.as_ref().and_then(|r| r.seed).unwrap_or(0);
    let mut rng = StdRng::seed_from_u64(seed);
    let mut trial_order: Vec<TrialSpec> = trials.to_vec();
    if let Some(ref spec) = randomization {
        shuffle_trials(&mut trial_order, spec, &mut rng);
    }
    let base_isi_ms = design
        .timing
        .as_ref()
        .and_then(|timing| timing.isi_ms)
        .unwrap_or(750.0);
    let isi_jitter_ms = design
        .timing
        .as_ref()
        .and_then(|timing| timing.isi_jitter_ms)
        .unwrap_or(0.0);
    let isi_base = base_isi_ms / 1000.0;
    let jitter_secs = isi_jitter_ms / 1000.0;
    let mut onset = 0.0;
    let mut events = Vec::new();
    for trial in &trial_order {
        let duration = trial.duration_ms / 1000.0;
        let cond = if trial.condition.is_empty() {
            "".into()
        } else {
            trial.condition.clone()
        };
        events.push(EventRow {
            onset,
            duration,
            trial: trial.trial,
            block: trial.block,
            event_type: "stim".into(),
            stim_id: trial.stim_id.clone(),
            condition: cond.clone(),
            resp_key: trial.resp_key.clone(),
            resp_rt: trial.resp_rt_ms.map(|ms| ms / 1000.0),
            value: trial.value.clone(),
        });
        if trial.resp_key.is_some() || trial.resp_rt_ms.is_some() || !trial.value.is_none() {
            events.push(EventRow {
                onset: onset + duration,
                duration: 0.0,
                trial: trial.trial,
                block: trial.block,
                event_type: "response".into(),
                stim_id: trial.stim_id.clone(),
                condition: cond.clone(),
                resp_key: trial.resp_key.clone(),
                resp_rt: trial.resp_rt_ms.map(|ms| ms / 1000.0),
                value: trial.value.clone(),
            });
        }
        let jitter_offset = if jitter_secs > 0.0 {
            rng.gen_range(-jitter_secs..=jitter_secs)
        } else {
            0.0
        };
        let isi = (isi_base + jitter_offset).max(0.0);
        onset += duration + isi;
    }
    let start_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs_f64())
        .unwrap_or(0.0);
    let manifest = RunManifest {
        sub: sub.to_string(),
        ses: ses.to_string(),
        run: run_id.to_string(),
        task: design.name.clone(),
        design: design.name.clone(),
        total_trials: trial_order.len(),
        total_events: events.len(),
        seed: randomization.as_ref().and_then(|r| r.seed),
        randomization_policy: randomization.as_ref().and_then(|spec| spec.policy.clone()),
        isi_ms: base_isi_ms,
        isi_jitter_ms: if isi_jitter_ms > 0.0 {
            Some(isi_jitter_ms)
        } else {
            None
        },
        start_time_unix: start_time,
    };
    RunBundle { events, manifest }
}

pub fn write_events_tsv(path: &Path, events: &[EventRow]) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut writer = WriterBuilder::new().delimiter(b'\t').from_writer(file);
    writer.write_record(&[
        "onset",
        "duration",
        "trial",
        "block",
        "event_type",
        "stim_id",
        "condition",
        "resp_key",
        "resp_rt",
        "value",
    ])?;
    for event in events {
        writer.write_record(&[
            event.onset.to_string(),
            event.duration.to_string(),
            event.trial.to_string(),
            event.block.to_string(),
            event.event_type.clone(),
            event.stim_id.clone(),
            event.condition.clone(),
            event.resp_key.clone().unwrap_or_else(|| "".into()),
            event
                .resp_rt
                .map(|v| v.to_string())
                .unwrap_or_else(|| "".into()),
            event.value.clone().unwrap_or_else(|| "".into()),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

pub fn write_events_json(path: &Path) -> Result<()> {
    let metadata = serde_json::json!({
        "columns": {
            "onset": { "units": "seconds" },
            "duration": { "units": "seconds" },
            "trial": {},
            "block": {},
            "event_type": {},
            "stim_id": {},
            "condition": {},
            "resp_key": {},
            "resp_rt": { "units": "seconds" },
            "value": {},
        }
    });
    fs::write(path, serde_json::to_string_pretty(&metadata)?)?;
    Ok(())
}

pub fn write_manifest(path: &Path, manifest: &RunManifest) -> Result<()> {
    let file = fs::File::create(path)?;
    serde_json::to_writer_pretty(file, manifest)?;
    Ok(())
}

pub fn read_manifest(path: &Path) -> Result<RunManifest> {
    let file =
        fs::File::open(path).with_context(|| format!("opening manifest {}", path.display()))?;
    let manifest = serde_json::from_reader::<_, RunManifest>(file)
        .with_context(|| format!("parsing manifest {}", path.display()))?;
    Ok(manifest)
}

pub fn read_events_tsv(path: &Path) -> Result<Vec<EventRow>> {
    let mut reader = ReaderBuilder::new()
        .delimiter(b'\t')
        .trim(Trim::All)
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("opening events {}", path.display()))?;
    let mut events = Vec::new();
    for row in reader.deserialize::<EventRow>() {
        let parsed = row.with_context(|| format!("parsing events in {}", path.display()))?;
        events.push(parsed);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn schedules_events_for_trials() {
        let design = DesignSpec {
            name: "test".into(),
            timing: Some(TimingSpec {
                isi_ms: Some(500.0),
                response_deadline_ms: None,
                isi_jitter_ms: None,
            }),
            randomization: Some(RandomizationSpec {
                policy: None,
                seed: Some(42),
            }),
        };
        let trials = vec![TrialSpec {
            trial: 1,
            block: 1,
            stim_id: "stim-A".into(),
            condition: "a".into(),
            duration_ms: 1000.0,
            resp_key: Some("J".into()),
            resp_rt_ms: Some(450.0),
            value: Some("1".into()),
        }];
        let bundle = simulate_run(&design, &trials, "01", "01", "01");
        assert_eq!(bundle.events.len(), 2);
        assert_eq!(bundle.events[0].event_type, "stim");
        assert_eq!(bundle.events[1].event_type, "response");
        assert_eq!(bundle.manifest.total_trials, 1);
        assert_eq!(bundle.manifest.total_events, 2);
    }

    #[test]
    fn writes_and_reads_events() {
        let dir = tempdir().unwrap();
        let event_path = dir.path().join("events.tsv");
        let events = vec![EventRow {
            onset: 0.0,
            duration: 0.8,
            trial: 1,
            block: 1,
            event_type: "stim".into(),
            stim_id: "foo".into(),
            condition: "bar".into(),
            resp_key: None,
            resp_rt: None,
            value: None,
        }];
        write_events_tsv(&event_path, &events).unwrap();
        let mut reader = ReaderBuilder::new()
            .delimiter(b'\t')
            .trim(Trim::All)
            .from_path(&event_path)
            .unwrap();
        let headers = reader.headers().unwrap().clone();
        assert!(headers.iter().any(|h| h == "onset"));
    }
}
