use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

/// Lightweight catalog summary for known runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub bundles: Vec<BundleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEntry {
    pub run_id: String,
    pub subject: String,
    pub session: String,
    pub task: String,
    pub design: String,
    pub total_trials: usize,
    pub total_events: usize,
    pub seed: Option<u64>,
    pub randomization_policy: Option<String>,
    pub isi_ms: f64,
    pub isi_jitter_ms: Option<f64>,
    pub started: f64,
    pub bundle_path: String,
    #[serde(skip)]
    pub path: PathBuf,
}

impl BundleEntry {
    fn from_manifest(manifest: BundleManifest, path: PathBuf) -> Self {
        Self {
            run_id: manifest.run,
            subject: manifest.sub,
            session: manifest.ses,
            task: manifest.task,
            design: manifest.design,
            total_trials: manifest.total_trials,
            total_events: manifest.total_events,
            seed: manifest.seed,
            randomization_policy: manifest.randomization_policy,
            isi_ms: manifest.isi_ms,
            isi_jitter_ms: manifest.isi_jitter_ms,
            started: manifest.start_time_unix,
            bundle_path: path.to_string_lossy().into_owned(),
            path,
        }
    }

    pub fn resource_uri(&self, file: &str) -> String {
        let file = file.trim_start_matches('/');
        if file.is_empty() {
            format!("elf://bundle/{}/events.tsv", self.run_id)
        } else {
            format!("elf://bundle/{}/{}", self.run_id, file)
        }
    }
}

#[derive(Debug, Deserialize)]
struct BundleManifest {
    sub: String,
    ses: String,
    run: String,
    task: String,
    design: String,
    total_trials: usize,
    total_events: usize,
    seed: Option<u64>,
    randomization_policy: Option<String>,
    isi_ms: f64,
    isi_jitter_ms: Option<f64>,
    start_time_unix: f64,
}

impl Catalog {
    /// Loads catalog index from disk (defaults to test_data/run_bundle).
    pub fn load() -> Result<Self> {
        let root = Catalog::bundle_root();
        let bundles = Catalog::scan(&root)?;
        Ok(Self { bundles })
    }

    fn bundle_root() -> PathBuf {
        env::var("ELF_BUNDLE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("test_data/run_bundle"))
    }

    fn scan(root: &Path) -> Result<Vec<BundleEntry>> {
        if !root.exists() {
            log::warn!("bundle root {} not found", root.display());
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();
        if root.join("run.json").exists() {
            candidates.push(root.to_path_buf());
        }
        if root.is_dir() {
            for entry in
                fs::read_dir(root).with_context(|| format!("reading {}", root.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && path.join("run.json").exists() {
                    candidates.push(path);
                }
            }
        }

        let mut seen = HashSet::new();
        let mut bundles = Vec::new();
        for dir in candidates.into_iter() {
            let canonical = dir.canonicalize().unwrap_or(dir.clone());
            if !seen.insert(canonical.clone()) {
                continue;
            }
            let manifest_path = canonical.join("run.json");
            match Catalog::load_manifest(&manifest_path) {
                Ok(manifest) => bundles.push(BundleEntry::from_manifest(manifest, canonical)),
                Err(err) => log::warn!("skipping bundle at {}: {}", manifest_path.display(), err),
            }
        }

        bundles.sort_by(|a, b| a.run_id.cmp(&b.run_id));
        Ok(bundles)
    }

    fn load_manifest(path: &Path) -> Result<BundleManifest> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading manifest {}", path.display()))?;
        let manifest: BundleManifest = serde_json::from_str(&contents)
            .with_context(|| format!("parsing manifest {}", path.display()))?;
        Ok(manifest)
    }

    pub fn first_bundle(&self) -> Option<&BundleEntry> {
        self.bundles.first()
    }

    pub fn by_run_id(&self, run_id: &str) -> Option<&BundleEntry> {
        self.bundles.iter().find(|bundle| bundle.run_id == run_id)
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({ "bundles": self.bundles, "count": self.bundles.len() })
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            bundles: Vec::new(),
        }
    }
}
