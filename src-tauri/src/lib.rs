pub mod ssh;
pub mod store;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

use ssh::{Hy2Config, Hy2User};
use store::{Meta, Settings};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    store::ensure_dir(&dir);
    Ok(dir)
}

#[derive(Serialize)]
struct ConfigEntry {
    name: String,
    link: String,
    description: String,
    created_ms: i64,
    expires_ms: Option<i64>,
    expired: bool,
}

#[derive(Serialize)]
struct ServerInfo {
    host: String,
    port: u16,
    sni: String,
    obfs: String,
    count: usize,
}

#[derive(Serialize)]
struct LoadResult {
    info: ServerInfo,
    entries: Vec<ConfigEntry>,
}

fn effective_sni(s: &Settings) -> String {
    if s.sni.trim().is_empty() {
        s.host.clone()
    } else {
        s.sni.trim().to_string()
    }
}

fn build_hy2_link(s: &Settings, cfg: &Hy2Config, u: &Hy2User) -> String {
    let mut query = format!("insecure=1&sni={}", effective_sni(s));
    if !cfg.obfs_type.is_empty() {
        query.push_str(&format!(
            "&obfs={}&obfs-password={}",
            cfg.obfs_type, cfg.obfs_password
        ));
    }
    format!(
        "hysteria2://{}:{}@{}:{}/?{}#{}",
        u.name, u.password, s.host, cfg.port, query, u.name
    )
}

/// If vless_link is configured, wraps hy2 + vless into a coffee://bundle.
/// Otherwise returns the plain hysteria2 link.
fn build_link(s: &Settings, cfg: &Hy2Config, u: &Hy2User) -> String {
    let hy2 = build_hy2_link(s, cfg, u);
    let vless = s.vless_link.trim();
    if vless.is_empty() {
        return hy2;
    }
    let w = URL_SAFE_NO_PAD.encode(hy2.as_bytes());
    let m = URL_SAFE_NO_PAD.encode(vless.as_bytes());
    format!("coffee://bundle?w={}&m={}", w, m)
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<Settings, String> {
    Ok(store::load_settings(&data_dir(&app)?))
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    store::save_settings(&data_dir(&app)?, &settings)
}

/// Connect, prune expired users, return the live list with metadata.
#[tauri::command]
fn load_configs(app: AppHandle) -> Result<LoadResult, String> {
    let dir = data_dir(&app)?;
    let settings = store::load_settings(&dir);
    if settings.host.trim().is_empty() {
        return Err("NO_SERVER".into());
    }
    if !ssh::hysteria_installed(&settings)? {
        return Err(format!(
            "На сервере не найден {} — Hysteria2 не установлен",
            ssh::HY_CONFIG
        ));
    }

    let mut meta = store::load_meta(&dir);
    let mut cfg = ssh::read_config(&settings)?;
    let now = now_ms();

    let expired: Vec<String> = cfg
        .users
        .iter()
        .filter(|u| {
            meta.get(&u.name)
                .and_then(|m| m.expires_ms)
                .map(|e| e <= now)
                .unwrap_or(false)
        })
        .map(|u| u.name.clone())
        .collect();

    if !expired.is_empty() {
        for name in &expired {
            ssh::remove_user(&settings, name)?;
            meta.remove(name);
        }
        store::save_meta(&dir, &meta)?;
        cfg = ssh::read_config(&settings)?;
    }

    let entries = cfg
        .users
        .iter()
        .map(|u| {
            let m = meta.get(&u.name).cloned().unwrap_or_default();
            ConfigEntry {
                name: u.name.clone(),
                link: build_link(&settings, &cfg, u),
                description: m.description,
                created_ms: m.created_ms,
                expires_ms: m.expires_ms,
                expired: false,
            }
        })
        .collect();

    Ok(LoadResult {
        info: ServerInfo {
            host: settings.host.clone(),
            port: cfg.port,
            sni: effective_sni(&settings),
            obfs: cfg.obfs_type.clone(),
            count: cfg.users.len(),
        },
        entries,
    })
}

/// Create a new config: random password, add on server, store metadata.
#[tauri::command]
fn create_config(
    app: AppHandle,
    name: String,
    description: String,
    ttl_days: i64,
) -> Result<ConfigEntry, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Укажи имя".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Имя: только латиница, цифры, _ и -".into());
    }

    let dir = data_dir(&app)?;
    let settings = store::load_settings(&dir);
    let password = random_password(16);
    ssh::add_user(&settings, &name, &password)?;

    let now = now_ms();
    let expires_ms = if ttl_days > 0 {
        Some(now + ttl_days * 86_400_000)
    } else {
        None
    };

    let mut meta = store::load_meta(&dir);
    meta.insert(
        name.clone(),
        Meta {
            description: description.trim().to_string(),
            created_ms: now,
            expires_ms,
        },
    );
    store::save_meta(&dir, &meta)?;

    let cfg = ssh::read_config(&settings)?;
    let user = Hy2User {
        name: name.clone(),
        password,
    };
    Ok(ConfigEntry {
        link: build_link(&settings, &cfg, &user),
        name,
        description: description.trim().to_string(),
        created_ms: now,
        expires_ms,
        expired: false,
    })
}

#[tauri::command]
fn delete_config(app: AppHandle, name: String) -> Result<(), String> {
    let dir = data_dir(&app)?;
    let settings = store::load_settings(&dir);
    ssh::remove_user(&settings, &name)?;
    let mut meta = store::load_meta(&dir);
    meta.remove(&name);
    store::save_meta(&dir, &meta)
}

fn random_password(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).expect("OS RNG unavailable");
    buf.iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            load_configs,
            create_config,
            delete_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
