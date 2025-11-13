use crate::{catalog::Catalog, docs::DocRegistry};
use anyhow::{anyhow, Context, Result};
use log::debug;
use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Resolves read-only resources exposed to MCP clients.
pub struct ResourceResolver {
    catalog: Catalog,
    bundles: HashMap<String, PathBuf>,
    docs: Arc<DocRegistry>,
    temp_paths: Mutex<HashMap<String, PathBuf>>,
}

impl ResourceResolver {
    pub fn new(catalog: &Catalog, docs: Arc<DocRegistry>) -> Self {
        let bundles = catalog
            .bundles
            .iter()
            .map(|entry| (entry.run_id.clone(), entry.path.clone()))
            .collect();
        Self {
            catalog: catalog.clone(),
            bundles,
            docs,
            temp_paths: Mutex::new(HashMap::new()),
        }
    }

    pub fn resolve(&self, uri: &str) -> Result<Resource> {
        if let Some(data) = self.resolve_temp(uri)? {
            return Ok(Resource {
                uri: uri.to_string(),
                data,
            });
        }

        if let Some(doc_data) = self.docs.resolve_uri(uri) {
            return Ok(Resource {
                uri: uri.to_string(),
                data: doc_data,
            });
        }

        if uri == "elf://catalog/index.json" {
            let payload = serde_json::to_vec_pretty(&self.catalog)?;
            return Ok(Resource {
                uri: uri.to_string(),
                data: payload,
            });
        }

        let prefix = "elf://bundle/";
        let remainder = uri
            .strip_prefix(prefix)
            .ok_or_else(|| anyhow!("expected {} prefix", prefix))?;

        let (run_id, mut file_path) = remainder
            .split_once('/')
            .map(|(run, path)| (run, path))
            .unwrap_or((remainder, "events.tsv"));
        if file_path.trim().is_empty() {
            file_path = "events.tsv";
        }

        let bundle_dir = self
            .bundles
            .get(run_id)
            .ok_or_else(|| anyhow!("unknown bundle {}", run_id))?;

        let safe_path = Self::sanitize_relative(file_path)?;
        let target = bundle_dir.join(safe_path);
        debug!("resolved {} -> {}", uri, target.display());
        let data =
            fs::read(&target).with_context(|| format!("reading resource {}", target.display()))?;

        Ok(Resource {
            uri: uri.to_string(),
            data,
        })
    }

    fn resolve_temp(&self, uri: &str) -> Result<Option<Vec<u8>>> {
        const TMP_PREFIX: &str = "elf://tmp/";
        if let Some(remainder) = uri.strip_prefix(TMP_PREFIX) {
            let (id, mut file_path) = remainder
                .split_once('/')
                .map(|(run, path)| (run, path))
                .unwrap_or((remainder, "events.tsv"));
            if file_path.trim().is_empty() {
                file_path = "events.tsv";
            }

            if let Ok(map) = self.temp_paths.lock() {
                if let Some(base_path) = map.get(id) {
                    let safe_path = Self::sanitize_relative(file_path)?;
                    let target = base_path.join(safe_path);
                    debug!("resolved {} -> {}", uri, target.display());
                    let data = fs::read(&target)
                        .with_context(|| format!("reading resource {}", target.display()))?;
                    return Ok(Some(data));
                }
            }
        }
        Ok(None)
    }

    pub fn register_temp_bundle(&self, id: &str, path: PathBuf) {
        if let Ok(mut map) = self.temp_paths.lock() {
            map.insert(id.to_string(), path);
        }
    }

    pub fn temp_base_path(&self, id: &str) -> Option<PathBuf> {
        self.temp_paths
            .lock()
            .ok()
            .and_then(|map| map.get(id).cloned())
    }

    fn sanitize_relative(path: &str) -> Result<PathBuf> {
        if path.starts_with('/') {
            anyhow::bail!("absolute paths are not allowed")
        }
        let candidate = Path::new(path);
        for component in candidate.components() {
            if matches!(component, Component::ParentDir) {
                anyhow::bail!("parent directory references are not allowed");
            }
            if matches!(component, Component::Prefix(_) | Component::RootDir) {
                anyhow::bail!("path component not allowed: {:?}", component);
            }
        }
        Ok(candidate.to_path_buf())
    }
}

pub struct Resource {
    pub uri: String,
    pub data: Vec<u8>,
}
