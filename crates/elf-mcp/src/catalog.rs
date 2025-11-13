use anyhow::Result;
use serde::{Deserialize, Serialize};

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
    pub started: String,
}

impl Catalog {
    /// Loads catalog index (currently a stub).
    pub fn load() -> Result<Self> {
        Ok(Self::default())
    }
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            bundles: Vec::new(),
        }
    }
}
