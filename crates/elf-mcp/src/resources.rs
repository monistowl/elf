use anyhow::{anyhow, Result};

/// Resolves read-only resources exposed to MCP clients.
pub struct ResourceResolver;

impl ResourceResolver {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve(&self, uri: &str) -> Result<Resource> {
        if uri.starts_with("elf://") {
            Ok(Resource {
                uri: uri.to_string(),
                data: Vec::new(),
            })
        } else {
            Err(anyhow!("uri must start with elf://"))
        }
    }
}

pub struct Resource {
    pub uri: String,
    pub data: Vec<u8>,
}
