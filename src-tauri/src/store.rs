//! File-backed persistence: server connection settings + per-config metadata
//! (description, created/expiry timestamps). Hysteria2 itself only stores
//! name→password, so descriptions and lifetimes live here.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// SSH connection + link-generation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub host: String,
    pub ssh_user: String,
    pub ssh_port: u16,
    /// SNI / masquerade domain embedded in generated links.
    pub sni: String,
    /// Optional explicit private key path; empty → use ssh defaults.
    pub key_path: String,
    /// Fallback VLESS Reality link for the mobile leg of coffee://bundle.
    /// Normally the per-user VLESS link is read straight from the x-ui inbound
    /// (matched to the hysteria2 user by name); this static link is only used
    /// for users with no matching x-ui client, or when x-ui is unreachable.
    #[serde(default)]
    pub vless_link: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            host: String::new(),
            ssh_user: "root".into(),
            ssh_port: 22,
            sni: String::new(),
            key_path: String::new(),
            vless_link: String::new(),
        }
    }
}

/// Local metadata for one config (keyed by user name).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Meta {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_ms: i64,
    /// Absolute expiry timestamp (ms). `None` = never expires.
    #[serde(default)]
    pub expires_ms: Option<i64>,
}

fn settings_path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}
fn meta_path(dir: &Path) -> PathBuf {
    dir.join("meta.json")
}

pub fn ensure_dir(dir: &Path) {
    let _ = fs::create_dir_all(dir);
}

pub fn load_settings(dir: &Path) -> Settings {
    fs::read_to_string(settings_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(dir: &Path, s: &Settings) -> Result<(), String> {
    ensure_dir(dir);
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    fs::write(settings_path(dir), json).map_err(|e| e.to_string())
}

pub fn load_meta(dir: &Path) -> HashMap<String, Meta> {
    fs::read_to_string(meta_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_meta(dir: &Path, m: &HashMap<String, Meta>) -> Result<(), String> {
    ensure_dir(dir);
    let json = serde_json::to_string_pretty(m).map_err(|e| e.to_string())?;
    fs::write(meta_path(dir), json).map_err(|e| e.to_string())
}
