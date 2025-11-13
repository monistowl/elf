use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use dirs_next::config_dir;
use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyUsagePurpose};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct KeyEntry {
    pub name: String,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub created: Option<String>,
}

impl KeyEntry {
    fn from_paths(name: String, cert_path: PathBuf, key_path: PathBuf) -> Self {
        let created = metadata_timestamp(&cert_path).or_else(|| metadata_timestamp(&key_path));
        Self {
            name,
            cert_path,
            key_path,
            created,
        }
    }
}

fn metadata_timestamp(path: &Path) -> Option<String> {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .map(|time| DateTime::<Utc>::from(time).to_rfc3339())
}

fn sanitize_name(name: &str) -> Result<String> {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        Err(anyhow!(
            "key name must contain at least one alphanumeric character"
        ))
    } else {
        Ok(sanitized)
    }
}

pub fn key_dir() -> Result<PathBuf> {
    if let Ok(dir) = env::var("ELF_KEY_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = config_dir().context("unable to locate config directory")?;
    Ok(base.join("elf-mcp/keys"))
}

fn cert_path(name: &str) -> Result<PathBuf> {
    Ok(key_dir()?.join(format!("{}.cert.pem", name)))
}

fn key_path(name: &str) -> Result<PathBuf> {
    Ok(key_dir()?.join(format!("{}.key.pem", name)))
}

fn ensure_dir() -> Result<PathBuf> {
    let dir = key_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn list_keys() -> Result<Vec<KeyEntry>> {
    let dir = ensure_dir()?;
    let mut certs = std::collections::HashMap::new();
    let mut keys = std::collections::HashMap::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(stem) = path.file_name().and_then(|os| os.to_str()) {
            if let Some(name) = stem.strip_suffix(".cert.pem") {
                certs.insert(name.to_string(), path);
            } else if let Some(name) = stem.strip_suffix(".key.pem") {
                keys.insert(name.to_string(), path);
            }
        }
    }
    let mut entries = Vec::new();
    for (name, cert_path) in certs.into_iter() {
        if let Some(key_path) = keys.remove(&name) {
            entries.push(KeyEntry::from_paths(name, cert_path, key_path));
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

pub fn generate_key(name: &str, _validity_days: u16) -> Result<KeyEntry> {
    let name = sanitize_name(name)?;
    let cert_file = cert_path(&name)?;
    let key_file = key_path(&name)?;
    if cert_file.exists() || key_file.exists() {
        return Err(anyhow!("key '{}' already exists", name));
    }
    ensure_dir()?;
    let mut params = CertificateParams::new(vec![name.clone()]);
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, name.clone());
    params.distinguished_name = dn;
    params.is_ca = IsCa::SelfSignedOnly;
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2099, 1, 1);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    let cert = Certificate::from_params(params)?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();
    fs::write(&cert_file, cert_pem)?;
    fs::write(&key_file, key_pem)?;
    Ok(KeyEntry::from_paths(name, cert_file, key_file))
}

pub fn import_key(name: &str, cert_src: &Path, key_src: &Path) -> Result<KeyEntry> {
    let name = sanitize_name(name)?;
    let cert_file = cert_path(&name)?;
    let key_file = key_path(&name)?;
    if cert_file.exists() || key_file.exists() {
        return Err(anyhow!("key '{}' already exists", name));
    }
    ensure_dir()?;
    fs::copy(cert_src, &cert_file)
        .with_context(|| format!("copying certificate to {}", cert_file.display()))?;
    fs::copy(key_src, &key_file)
        .with_context(|| format!("copying private key to {}", key_file.display()))?;
    Ok(KeyEntry::from_paths(name, cert_file, key_file))
}

pub fn export_key(name: &str, destination: &Path) -> Result<(PathBuf, PathBuf)> {
    let entry = find_key(name)?;
    let cert_dest = destination.join(format!("{}.cert.pem", name));
    let key_dest = destination.join(format!("{}.key.pem", name));
    fs::copy(&entry.cert_path, &cert_dest)
        .with_context(|| format!("exporting cert to {}", cert_dest.display()))?;
    fs::copy(&entry.key_path, &key_dest)
        .with_context(|| format!("exporting key to {}", key_dest.display()))?;
    Ok((cert_dest, key_dest))
}

pub fn find_key(name: &str) -> Result<KeyEntry> {
    let entries = list_keys()?;
    entries
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| anyhow!("key '{}' not found", name))
}
