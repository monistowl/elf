use crate::{catalog::Catalog, docs::DocRegistry};
use anyhow::{anyhow, Context, Result};
use log::debug;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Resolves read-only resources exposed to MCP clients.
pub struct ResourceResolver {
    catalog: Catalog,
    bundles: HashMap<String, PathBuf>,
    docs: Arc<DocRegistry>,
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
        }
    }

    pub fn resolve(&self, uri: &str) -> Result<Resource> {
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
